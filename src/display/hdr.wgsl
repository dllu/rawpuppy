struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}
@group(0) @binding(0) var photo: texture_2d<f32>;
@group(0) @binding(1) var filtering: sampler;
@group(0) @binding(2) var<uniform> display_white: vec4<f32>;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Vertex {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Vertex;
    out.position = vec4<f32>(corner * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = corner;
    return out;
}

@fragment
fn fragment(in: Vertex) -> @location(0) vec4<f32> {
    let pixel = textureSample(photo, filtering, in.uv);
    let rgb = clamp(pixel.rgb * display_white.x, vec3<f32>(-65504.0), vec3<f32>(65504.0));
    return vec4<f32>(rgb, pixel.a);
}
