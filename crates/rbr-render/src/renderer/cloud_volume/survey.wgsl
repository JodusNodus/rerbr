// Equal solid-angle samples of the upper hemisphere: azimuth and sine of elevation are uniform. No terrain,
// camera projection, history or post-processing. Both layers use the renderer's own marches and composition.
@fragment
fn fs_cloud_survey(v: SkyOut) -> @location(0) vec4<f32> {
    let uv = vec2<f32>(v.ndc.x, -v.ndc.y) * 0.5 + 0.5;
    let y = uv.y;
    let r = sqrt(max(1.0 - y * y, 0.0));
    let angle = uv.x * 2.0 * SKY_PI;
    let dir = vec3<f32>(r * cos(angle), y, r * sin(angle));
    let low = volume_march(dir, v.clip.xy);
    let high = cirrus(dir);
    return vec4<f32>(1.0 - low.a, 1.0 - high.a, 1.0 - low.a * high.a, 1.0);
}
