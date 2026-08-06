//! DMX output over Art-Net and sACN (E1.31).
//!
//! PLAN.md Phase 5 lists DMX as show-I/O table stakes, alongside Spout and
//! NDI. Unlike those, it needs no vendor SDK: both protocols are UDP packets
//! with a documented layout, which means the whole thing can be built and —
//! more importantly — *tested* without a lighting rig in the room.
//!
//! Channels become DMX slots in order. A channel's samples are not sent
//! individually: DMX is a state protocol, so each frame sends the current
//! value of each channel, which is the last sample of the slice.

/// Art-Net's fixed UDP port.
pub const ARTNET_PORT: u16 = 6454;
/// sACN's fixed UDP port.
pub const SACN_PORT: u16 = 5568;

/// A DMX universe: 512 slots of 8-bit level.
pub const UNIVERSE_SIZE: usize = 512;

/// Build an Art-Net `OpDmx` packet.
///
/// Layout from the Art-Net 4 specification: an 8-byte magic, a little-endian
/// opcode, a big-endian protocol version, sequence and physical bytes, a
/// little-endian universe, then a big-endian length and the slot data.
/// The mixed endianness is not a mistake in this code — it is in the spec.
pub fn artnet_packet(universe: u16, sequence: u8, slots: &[u8]) -> Vec<u8> {
    let length = slots.len().min(UNIVERSE_SIZE);
    // The spec requires an even length of at least 2.
    let padded = length.max(2).next_multiple_of(2);

    let mut p = Vec::with_capacity(18 + padded);
    p.extend_from_slice(b"Art-Net\0");
    p.extend_from_slice(&0x5000u16.to_le_bytes()); // OpDmx
    p.extend_from_slice(&14u16.to_be_bytes()); // protocol version
    p.push(sequence);
    p.push(0); // physical port, informational only
    p.extend_from_slice(&universe.to_le_bytes());
    p.extend_from_slice(&(padded as u16).to_be_bytes());
    p.extend_from_slice(&slots[..length]);
    p.resize(18 + padded, 0);
    p
}

/// Build an sACN (E1.31) data packet.
///
/// Three nested PDUs — root, framing, DMP — each carrying a flags-and-length
/// field where the top four bits are `0x7`. The layout is fiddly and the
/// offsets are easy to get wrong, which is exactly why this is a pure
/// function with a test against known byte positions.
pub fn sacn_packet(universe: u16, sequence: u8, source: &str, cid: &[u8; 16], slots: &[u8]) -> Vec<u8> {
    let count = slots.len().min(UNIVERSE_SIZE);
    // The DMP layer carries a start code byte before the slots.
    let dmp_property_count = count + 1;

    let mut p = Vec::with_capacity(126 + count);

    // ---- root layer
    p.extend_from_slice(&0x0010u16.to_be_bytes()); // preamble size
    p.extend_from_slice(&0x0000u16.to_be_bytes()); // postamble size
    p.extend_from_slice(b"ASC-E1.17\0\0\0"); // ACN packet identifier
    // Each PDU length counts from its own length field to the end of the
    // packet. Getting these wrong is the classic sACN bug: receivers drop
    // the packet silently, so the test checks them against real offsets.
    let root_length = 0x7000 | (110 + count) as u16;
    p.extend_from_slice(&root_length.to_be_bytes());
    p.extend_from_slice(&0x0000_0004u32.to_be_bytes()); // VECTOR_ROOT_E131_DATA
    p.extend_from_slice(cid);

    // ---- framing layer
    let framing_length = 0x7000 | (88 + count) as u16;
    p.extend_from_slice(&framing_length.to_be_bytes());
    p.extend_from_slice(&0x0000_0002u32.to_be_bytes()); // VECTOR_E131_DATA_PACKET
    let mut name = [0u8; 64];
    for (i, b) in source.bytes().take(63).enumerate() {
        name[i] = b;
    }
    p.extend_from_slice(&name);
    p.push(100); // priority
    p.extend_from_slice(&0u16.to_be_bytes()); // synchronization address
    p.push(sequence);
    p.push(0); // options
    p.extend_from_slice(&universe.to_be_bytes());

    // ---- DMP layer
    let dmp_length = 0x7000 | (10 + dmp_property_count) as u16;
    p.extend_from_slice(&dmp_length.to_be_bytes());
    p.push(0x02); // VECTOR_DMP_SET_PROPERTY
    p.push(0xa1); // address type and data type
    p.extend_from_slice(&0x0000u16.to_be_bytes()); // first property address
    p.extend_from_slice(&0x0001u16.to_be_bytes()); // address increment
    p.extend_from_slice(&(dmp_property_count as u16).to_be_bytes());
    p.push(0x00); // DMX start code
    p.extend_from_slice(&slots[..count]);
    p
}

/// The multicast address sACN uses for a universe.
pub fn sacn_multicast(universe: u16) -> std::net::Ipv4Addr {
    let [hi, lo] = universe.to_be_bytes();
    std::net::Ipv4Addr::new(239, 255, hi, lo)
}

/// Map a channel value to a DMX slot.
///
/// Values are expected in 0..1 — the range every other operator works in —
/// and clamped rather than wrapped, because a light jumping from full to
/// black on an overshoot is far worse than one that simply saturates.
pub fn to_slot(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_artnet_packet_has_the_documented_layout() {
        let p = artnet_packet(3, 7, &[255, 0, 128]);
        assert_eq!(&p[0..8], b"Art-Net\0");
        // Opcode is little-endian, protocol version big-endian.
        assert_eq!(&p[8..10], &[0x00, 0x50]);
        assert_eq!(&p[10..12], &[0x00, 0x0e]);
        assert_eq!(p[12], 7, "sequence");
        assert_eq!(&p[14..16], &[3, 0], "universe, little-endian");
        assert_eq!(&p[16..18], &[0, 4], "length, big-endian and even");
        assert_eq!(&p[18..22], &[255, 0, 128, 0]);
    }

    #[test]
    fn an_artnet_packet_is_never_odd_or_shorter_than_two() {
        assert_eq!(artnet_packet(0, 0, &[]).len(), 18 + 2);
        assert_eq!(artnet_packet(0, 0, &[1]).len(), 18 + 2);
        assert_eq!(artnet_packet(0, 0, &[1, 2, 3]).len(), 18 + 4);
        // And a universe cannot exceed 512 slots however many channels exist.
        let big = vec![1u8; 900];
        assert_eq!(artnet_packet(0, 0, &big).len(), 18 + 512);
    }

    #[test]
    fn an_sacn_packet_has_the_documented_layout() {
        let cid = [0xab; 16];
        let p = sacn_packet(1, 5, "otd", &cid, &[10, 20, 30]);
        assert_eq!(&p[0..2], &[0x00, 0x10], "preamble");
        assert_eq!(&p[4..16], b"ASC-E1.17\0\0\0");
        assert_eq!(&p[22..38], &cid);
        assert_eq!(&p[44..47], b"otd");
        assert_eq!(p[108], 100, "priority");
        assert_eq!(p[111], 5, "sequence");
        assert_eq!(&p[113..115], &[0, 1], "universe, big-endian");
        assert_eq!(p[125], 0x00, "DMX start code");
        assert_eq!(&p[126..129], &[10, 20, 30]);

        // Every PDU length carries the 0x7 flags nibble and counts from its
        // own field to the end of the packet.
        for (offset, label) in [(16usize, "root"), (38, "framing"), (115, "dmp")] {
            let len = u16::from_be_bytes([p[offset], p[offset + 1]]);
            assert_eq!(len >> 12, 0x7, "{label} flags");
            assert_eq!(
                len & 0x0fff,
                (p.len() - offset) as u16,
                "{label} length"
            );
        }
    }

    #[test]
    fn sacn_multicast_follows_the_universe() {
        assert_eq!(
            sacn_multicast(1),
            std::net::Ipv4Addr::new(239, 255, 0, 1)
        );
        assert_eq!(
            sacn_multicast(300),
            std::net::Ipv4Addr::new(239, 255, 1, 44)
        );
    }

    #[test]
    fn levels_saturate_rather_than_wrapping() {
        assert_eq!(to_slot(0.0), 0);
        assert_eq!(to_slot(1.0), 255);
        assert_eq!(to_slot(0.5), 128);
        // A light going full-to-black on an overshoot is much worse than one
        // that simply stays at full.
        assert_eq!(to_slot(1.7), 255);
        assert_eq!(to_slot(-0.3), 0);
        assert_eq!(to_slot(f32::NAN), 0);
    }
}
