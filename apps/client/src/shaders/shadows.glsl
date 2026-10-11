uniform sampler2D ark_shadow_map;
uniform mat4 ark_shadow_matrix;
uniform float ark_shadow_enabled;
uniform float ark_shadow_texel;
float ark_sun_visibility(vec3 p, vec3 n, vec3 l) {
    if(ark_shadow_enabled<0.5) return 1.0;
    vec3 q=(ark_shadow_matrix*vec4(p,1.0)).xyz*0.5+0.5;
    if(any(lessThanEqual(q,vec3(0.0))) || any(greaterThanEqual(q,vec3(1.0)))) return 1.0;
    // Normal-dependent depth bias and a bounded 3x3 PCF kernel. Edges fade into
    // unshadowed distant geometry instead of showing a camera-following square.
    float bias=max(0.000002,0.000025*(1.0-max(dot(n,l),0.0)));
    float visible=0.0;
    for(int y=-1;y<=1;y++) for(int x=-1;x<=1;x++) {
        float z=texture(ark_shadow_map,q.xy+vec2(float(x),float(y))*ark_shadow_texel).r;
        visible+=step(q.z-bias,z);
    }
    float edge=min(min(q.x,1.0-q.x),min(q.y,1.0-q.y));
    return mix(1.0,visible/9.0,smoothstep(0.0,0.08,edge));
}
