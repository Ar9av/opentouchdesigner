// Dense local Lucas-Kanade least squares over a 5x5 neighbourhood.
// https://docs.opencv.org/4.x/d4/dee/tutorial_optical_flow.html
// Both inputs must be the same size. Positive XY follows motion in texture UV.
fn luma0(uv: vec2<f32>) -> f32 {
    return dot(sample0(uv).rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
}
fn luma1(uv: vec2<f32>) -> f32 {
    return dot(sample1(uv).rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
}
@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let dx = vec2<f32>(U.res.z, 0.0);
    let dy = vec2<f32>(0.0, U.res.w);
    var xx = 0.0; var xy = 0.0; var yy = 0.0;
    var xt = 0.0; var yt = 0.0;
    for (var y = -2; y <= 2; y = y + 1) {
        for (var x = -2; x <= 2; x = x + 1) {
            let p = in.uv + vec2<f32>(f32(x), f32(y)) * U.res.zw;
            let gx = (luma0(p + dx) - luma0(p - dx)
                    + luma1(p + dx) - luma1(p - dx)) * 0.25;
            let gy = (luma0(p + dy) - luma0(p - dy)
                    + luma1(p + dy) - luma1(p - dy)) * 0.25;
            let dt = luma0(p) - luma1(p);
            xx += gx * gx; xy += gx * gy; yy += gy * gy;
            xt += gx * dt; yt += gy * dt;
        }
    }
    let regularization = max(U.p0.y, 0.000001);
    xx += regularization; yy += regularization;
    let determinant = max(xx * yy - xy * xy, regularization * regularization);
    var motion = vec2<f32>(xy * yt - yy * xt, xy * xt - xx * yt) / determinant;
    let limit = max(U.p0.z, 0.1);
    let speed = length(motion);
    motion *= min(1.0, limit / max(speed, 0.000001));
    return vec4<f32>(vec2<f32>(0.5) + motion * U.res.zw * U.p0.x,
                     min(speed / limit, 1.0), 1.0);
}
