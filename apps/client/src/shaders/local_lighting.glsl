// Scene-relative units are 100 metres. Fixed loop is valid in WebGL2/GLSL150.
const int ARK_LOCAL_LIGHT_MAX = 8;
uniform int ark_local_count;
uniform vec4 ark_local_position_range[ARK_LOCAL_LIGHT_MAX];
uniform vec4 ark_local_color_power[ARK_LOCAL_LIGHT_MAX];
vec3 ark_local_lighting(vec3 world, vec3 n, vec3 v, vec3 albedo, float roughness, float metallic) {
    vec3 result = vec3(0.0);
    for (int i=0; i<ARK_LOCAL_LIGHT_MAX; ++i) {
        if (i >= ark_local_count) break;
        vec3 offset = ark_local_position_range[i].xyz - world;
        float distance2 = dot(offset,offset);
        float range = max(ark_local_position_range[i].w,0.001);
        float normalized2 = distance2/(range*range);
        // Compact smooth support: zero radiance and zero slope at the radius.
        float window = max(1.0-normalized2*normalized2,0.0);
        window *= window;
        float falloff = window/(1.0+36.0*normalized2);
        vec3 l = offset*inversesqrt(max(distance2,0.000001));
        vec3 radiance = ark_local_color_power[i].rgb * ark_local_color_power[i].a * falloff;
        result += ark_brdf(n,v,l,albedo,roughness,metallic)*radiance;
    }
    return result;
}
