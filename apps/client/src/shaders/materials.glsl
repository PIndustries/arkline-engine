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
