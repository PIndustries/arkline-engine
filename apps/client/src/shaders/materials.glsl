float ark_hash3(vec3 p){return fract(sin(dot(p,vec3(127.1,311.7,74.7)))*43758.5453);}
float ark_noise3(vec3 p){vec3 i=floor(p),f=fract(p);f=f*f*(3.0-2.0*f);
return mix(mix(mix(ark_hash3(i),ark_hash3(i+vec3(1,0,0)),f.x),mix(ark_hash3(i+vec3(0,1,0)),ark_hash3(i+vec3(1,1,0)),f.x),f.y),
mix(mix(ark_hash3(i+vec3(0,0,1)),ark_hash3(i+vec3(1,0,1)),f.x),mix(ark_hash3(i+vec3(0,1,1)),ark_hash3(i+vec3(1,1,1)),f.x),f.y),f.z);}

// Single-scatter GGX dielectric/conductor response. Metallic surfaces have no
// diffuse lobe; Fresnel removes reflected energy from the dielectric diffuse term.
vec3 ark_fresnel(float cosine,vec3 f0){return f0+(1.0-f0)*pow(1.0-clamp(cosine,0.0,1.0),5.0);}
vec3 ark_brdf(vec3 n,vec3 v,vec3 l,vec3 albedo,float roughness,float metallic){
    vec3 h=normalize(v+l+vec3(1e-7));
    float nv=max(dot(n,v),0.001),nl=max(dot(n,l),0.0),nh=max(dot(n,h),0.0),vh=max(dot(v,h),0.0);
    float a=max(roughness*roughness,0.0064),a2=a*a,d=nh*nh*(a2-1.0)+1.0;
    float distribution=a2/max(ARK_PI*d*d,1e-7);
    // Height-correlated Smith GGX visibility, includes the 1/(4*Nv*Nl) term.
    float gv=nl*sqrt(nv*nv*(1.0-a2)+a2),gl=nv*sqrt(nl*nl*(1.0-a2)+a2);
    float visibility=0.5/max(gv+gl,1e-5);
    vec3 f0=mix(vec3(0.04),albedo,metallic);
    vec3 fresnel=ark_fresnel(vh,f0);
    return ((1.0-fresnel)*(1.0-metallic)*albedo/ARK_PI+distribution*visibility*fresnel)*nl;
}
vec3 ark_environment(vec3 n,vec3 v,vec3 albedo,float roughness,float metallic,float atmosphere,vec3 up){
    vec3 average=mix(vec3(0.040,0.057,0.096),vec3(0.18,0.30,0.39),atmosphere);
    vec3 reflected=ark_sky_radiance(reflect(-v,n),atmosphere,up);
    // Low-cost lobe broadening: sharp directional sky fades toward hemispheric mean.
    vec3 specular_environment=mix(reflected,average,roughness*roughness);
    vec3 f0=mix(vec3(0.04),albedo,metallic);
    vec3 fresnel=f0+(max(vec3(1.0-roughness),f0)-f0)*pow(1.0-max(dot(n,v),0.0),5.0);
    vec3 diffuse_environment=mix(average*0.72,average*1.28,clamp(dot(n,up)*0.5+0.5,0.0,1.0));
    return specular_environment*fresnel+(1.0-fresnel)*(1.0-metallic)*albedo*diffuse_environment;
}

// Analytic approximation of the enclosed dock's illuminated ceiling and walls.
// This is a reflection environment, not a ray trace of nearby ships. Direct
// fixture lights are supplied separately by ark_local_lighting.
vec3 ark_station_radiance(vec3 direction,float roughness){
    float ceiling=smoothstep(-0.10,0.75,direction.y);
    vec3 wall=mix(vec3(0.075,0.095,0.105),vec3(0.24,0.29,0.32),ceiling);
    float width=mix(0.055,0.40,roughness*roughness);
    float strip=exp(-pow((abs(direction.z)-0.48)/width,2.0));
    float ceiling_panel=strip*smoothstep(0.05,0.60,direction.y);
    // Broader rough reflections have a lower peak instead of adding energy.
    return wall+vec3(0.82,0.94,1.0)*ceiling_panel*mix(1.8,0.32,roughness);
}
vec3 ark_station_environment(vec3 n,vec3 v,vec3 albedo,float roughness,float metallic){
    roughness=clamp(roughness,0.08,1.0);metallic=clamp(metallic,0.0,1.0);
    vec3 reflected=ark_station_radiance(reflect(-v,n),roughness);
    vec3 f0=mix(vec3(0.04),albedo,metallic);
    vec3 f=ark_fresnel(max(dot(n,v),0.0),f0);
    vec3 irradiance=mix(vec3(0.18,0.21,0.23),vec3(0.65,0.74,0.79),clamp(n.y*0.5+0.5,0.0,1.0));
    return reflected*f+(1.0-f)*(1.0-metallic)*albedo*irradiance;
}

// Object-space procedural finish for opaque authored hulls. This modulates
// material response only: it adds no geometry, damage, normal map or fake light.
// Fade subpixel grain rather than letting fine detail shimmer in distant views.
void ark_hull_surface(vec3 meters,inout vec3 albedo,inout float roughness,float metallic){
    float broad=ark_noise3(meters*0.63)-0.5;
    float footprint=max(length(fwidth(meters)),0.0001);
    float resolved=1.0-smoothstep(0.025,0.15,footprint);
    float grain=(ark_noise3(meters*vec3(3.0,18.0,18.0))-0.5)*resolved;
    albedo*=clamp(1.0+broad*0.12+grain*0.04,0.90,1.10);
    roughness=clamp(roughness+broad*0.07+grain*mix(0.035,0.08,metallic),0.08,1.0);
}

// Metric construction details, antialiased with actual fragment footprint.
// No random grunge or light is added; normals/geometry remain authored.
void ark_interior_surface(vec2 meters,float role,inout vec3 albedo,inout float roughness){
    float footprint=max(length(fwidth(meters)),0.0001);
    if(role<2.5){
        vec2 seam=abs(fract(meters/vec2(1.2,0.8)+0.5)-0.5)*vec2(1.2,0.8);
        float joint=1.0-smoothstep(0.004,0.004+footprint,min(seam.x,seam.y));
        albedo*=mix(1.0,0.28,joint);roughness=mix(roughness,0.86,joint);
        // Protective lower-band cladding is modelled separately, not inferred here.
    }else if(role<3.5){
        float resolved=1.0-smoothstep(0.02,0.07,footprint);
        vec2 q=meters*vec2(25.0,25.0);
        float grip=1.0-smoothstep(0.08,0.18,abs(fract(q.x+q.y)-0.5));
        vec2 seam=abs(fract(meters/0.8+0.5)-0.5)*0.8;
        float joint=1.0-smoothstep(0.004,0.004+footprint,min(seam.x,seam.y));
        albedo*=mix(1.0,1.22,grip*resolved)*mix(1.0,0.3,joint);roughness=0.9;
    }else{
        float slots=1.0-smoothstep(0.003,0.003+footprint,abs(fract(meters.x/0.10+0.5)-0.5)*0.10);
        albedo*=mix(1.0,0.5,slots);roughness=0.82;
    }
}
