uniform sampler2D ark_shadow_map;
uniform mat4 ark_shadow_matrix;
uniform float ark_shadow_enabled;
uniform float ark_shadow_texel;
float ark_sun_visibility(vec3 p, vec3 n, vec3 l) {
    if(ark_shadow_enabled<0.5) return 1.0;
    vec3 q=(ark_shadow_matrix*vec4(p,1.0)).xyz*0.5+0.5;
    // Reconstruct the receiver plane in shadow UV space before divergent edge
    // exits. Each PCF tap compares against the plane at its own texel center;
    // comparing every tap to q.z creates diagonal self-shadow stripes on slopes.
    vec3 dx=dFdx(q),dy=dFdy(q);
    float determinant=dx.x*dy.y-dx.y*dy.x;
    bool plane_valid=abs(determinant)>1e-12;
    vec2 gradient=plane_valid ? vec2(dy.y*dx.z-dx.y*dy.z,dx.x*dy.z-dy.x*dx.z)/determinant : vec2(0.0);
    if(any(lessThanEqual(q,vec3(0.0))) || any(greaterThanEqual(q,vec3(1.0)))) return 1.0;
    // Map width/depth = 48/640. Bias scales with texel footprint, including
    // lower-resolution remote feeds. A conservative slope fallback covers
    // degenerate projected derivatives without weakening distant occluders.
    float nl=clamp(dot(n,l),0.0,1.0);
    float slope=sqrt(max(0.0,1.0-nl*nl))/max(nl,0.15);
    // Three quarters of a texel also covers taps crossing adjacent low-poly
    // facets, where a single receiver plane no longer describes the blocker.
    // Bounded to ~3.5m at1024/~1.8m at2048; no global brightness change.
    float bias=max(0.000002,ark_shadow_texel*0.075*(plane_valid ? 0.75 : (0.75+1.5*slope)));
    float visible=0.0;
    for(int y=-1;y<=1;y++) for(int x=-1;x<=1;x++) {
        vec2 uv=q.xy+vec2(float(x),float(y))*ark_shadow_texel;
        vec2 center=(floor(uv/ark_shadow_texel)+0.5)*ark_shadow_texel;
        float z=texture(ark_shadow_map,center).r;
        float reference=q.z+dot(gradient,center-q.xy)-bias;
        visible+=step(reference,z);
    }
    float edge=min(min(q.x,1.0-q.x),min(q.y,1.0-q.y));
    return mix(1.0,visible/9.0,smoothstep(0.0,0.08,edge));
}
