// Shared linear sky field. Environment lighting approximates this distant radiance,
// not reflections of nearby geometry. Sun is handled separately as a direct light.
const float ARK_PI = 3.14159265359;
vec2 ark_sky_uv(vec3 d){return vec2(atan(d.z,d.x)/(2.0*ARK_PI)+0.5,asin(clamp(d.y,-1.0,1.0))/ARK_PI+0.5);}
vec3 ark_space_radiance(vec3 d){
    vec2 p=ark_sky_uv(d);
    float band=exp(-pow((p.y-0.4+sin(p.x*3.4)*0.16)*3.0,2.0));
    float sweep=0.5+0.5*sin(p.x*6.2831853+0.8);
    return vec3(0.008,0.014,0.032)+band*mix(vec3(0.026,0.045,0.080),vec3(0.058,0.067,0.125),sweep);
}
vec3 ark_sky_radiance(vec3 d,float atmosphere,vec3 up){
    float horizon=pow(1.0-abs(dot(d,up)),3.0);
    vec3 sky=mix(vec3(0.065,0.17,0.29),vec3(0.44,0.55,0.60),horizon);
    return mix(ark_space_radiance(d),sky,atmosphere);
}
// One linear->filmic->sRGB conversion; callback disables FRAMEBUFFER_SRGB.
vec3 ark_display(vec3 x){x=max(x,vec3(0.0))*1.12;vec3 film=clamp((x*(2.51*x+0.03))/(x*(2.43*x+0.59)+0.14),0.0,1.0);return mix(film*12.92,1.055*pow(film,vec3(1.0/2.4))-0.055,step(vec3(0.0031308),film));}
