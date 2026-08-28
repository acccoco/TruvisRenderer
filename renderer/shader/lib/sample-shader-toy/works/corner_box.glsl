const int SAMPLES_PER_PIXEL = 100;
const int BOUNCES = 3;
const float P_TERMINATE = 0.75;
const float EPSILON = 0.01;

#define PI 3.14159265
#define saturate(x) clamp(x, 0.0, 1.0)

// sRGB, linear space conversions
#define stol1(x) (x <= 0.04045 ? x / 12.92 : pow((x + 0.055) / 1.055, 2.4))
#define stol3(x, y, z) vec3(stol1(x), stol1(y), stol1(z))
#define ltos1(x) (x <= 0.0031308 ? x * 12.92 : 1.055 * pow(x, 0.4166667) - 0.055)
#define ltos3(x, y, z) vec3(ltos1(x), ltos1(y), ltos1(z))

// From iq
float seed = 1.0; //seed initialized in main
float rnd() { return fract(sin(seed++)*43758.5453123); }

// From hg_sdf
void pR(inout vec2 p, float a) {
    p = cos(a)*p + sin(a)*vec2(p.y, -p.x);
}

// --STRUCTS-------------------------------------------------------------------
struct AreaLight {
    mat4 toWorld;
    vec2 size;
    vec3 E;
};

struct Material {
    vec3 albedo;
    float roughness;
    float metalness;
    vec3 emission;
};

struct Ray {
    vec3 o;
    vec3 d;
    float t;
};

struct Hit {
    bool hit;
    vec3 position;
    vec3 normal;
    Material material;
};

struct RDir {
    vec3 d;
    float pdf;
};

// --SCENE---------------------------------------------------------------------
const int NUM_LIGHTS = 1;
const AreaLight LIGHTS[] = AreaLight[](
    AreaLight(mat4(1, 0, 0, 0,
                   0, 1, 0, 0,
                   0, 0, 1, 0,
                   0, 5, 0, 1),
              vec2(1),
              vec3(0.85, 0.8, 0.4) * vec3(30))
);

const int NUM_SPHERES = 7;
const vec4 SPHERES[] = vec4[](
    vec4(  -1.5,     -3,     0,     2),
    vec4(     1,     -4,    -2,     1),
    vec4(     0, -10005,     0, 10000),
    vec4(     0,      0, 10005, 10000),
    vec4(     0,  10005,     0, 10000),
    vec4(-10005,      0,     0, 10000),
    vec4( 10005,      0,     0, 10000)
);

const vec3 COLORS[] = vec3[](
    vec3(0, 0, 1),
    stol3(1.0, 0.863, 0.616),
    vec3(180) / vec3(255),
    vec3(180) / vec3(255),
    vec3(180) / vec3(255),
    vec3(180, 0, 0) / vec3(255),
    vec3(0, 180, 0) / vec3(255)
);

Material evalMaterial(vec3 p, int i)
{
    Material m;
    m.albedo = vec3(1, 0, 1);
    m.roughness = 1.0;
    m.metalness = 0.0;
    m.emission = vec3(1, 0, 1);
    if (i >= 0) {
        m.albedo = COLORS[i];
        m.emission = vec3(0.0);
        if (i == 0) {
            m.roughness = 0.3;
            m.metalness = 0.0;
        } else if (i == 1) {
            m.roughness = 0.4;
            m.metalness = 1.0;
        } else if (i == 4 && all(lessThan(abs(p.xz), LIGHTS[0].size))) {
            m.emission = LIGHTS[0].E;
        }
    }
    return m;
}

// --GEOMETRIC-----------------------------------------------------------------
// Generate basis matrix for given normal
mat3 formBasis(vec3 n)
{
    // Make vector q that is non-parallel to n
    vec3 q = n;
    vec3 aq = abs(q);
    if (aq.x <= aq.y && aq.x <= aq.z) {
        q.x = 1.f;
    } else if (aq.y <= aq.x && aq.y <= aq.z) {
        q.y = 1.f;
    } else {
        q.z = 1.f;
    }

    // Generate two vectors perpendicular to n
    vec3 t = normalize(cross(q, n));
    vec3 b = normalize(cross(n, t));

    // Construct the rotation matrix
    mat3 m;
    m[0] = t;
    m[1] = b;
    m[2] = n;
    return m;
}

// Generate view-ray for given (sub)pixel
vec3 getViewRay(vec2 px, float hfov)
{
    vec2 xy = px - iResolution.xy * 0.5;
    float z = iResolution.y / tan(radians(hfov));
    vec3 d = normalize(vec3(xy, z));
    if (iMouse.x > 0.0) {// Don't use the initial mouse position
    	pR(d.yz, iMouse.y / iResolution.y * 2.0 - 1.0);
    	pR(d.xz, iMouse.x / iResolution.x * 2.0 - 1.0);
    }
    return d;
}

// --SAMPLING------------------------------------------------------------------
vec4 sampleLight(int i)
{
    AreaLight light = LIGHTS[i];
    float pdf = 1.0 / (4.0 * light.size.x * light.size.y);
    mat4 S = mat4(light.size.x,            0, 0, 0,
                            0, light.size.y, 0, 0,
                            0,            0, 1, 0,
                            0,            0, 0, 1);
    mat4 M = light.toWorld * S;
    return vec4((M * vec4(vec2(rnd(), rnd()) * 2.0 - 1.0, 0, 1)).xyz, pdf);
}

// From http://www.rorydriscoll.com/2009/01/07/better-sampling/
vec3 cosineSampleHemisphere() {
    vec2 u = vec2(rnd(), rnd());
    float r = sqrt(u.x);
    float theta = 2.0 * PI * u.y;
    return vec3(r * cos(theta), r * sin(theta), sqrt(saturate(1.0 - u.x)));
}

float cosineHemispherePDF(float NoL)
{
    return NoL / PI;
}

// --SHADING-------------------------------------------------------------------
// Lambert diffuse term
vec3 lambertBRFD(vec3 albedo)
{
    return albedo / PI;
}

// GGX distribution function
float ggx(float NoH, float roughness)
{
    float a2 = roughness * roughness;
    a2 *= a2;
    float denom = NoH * NoH * (a2 - 1.0) + 1.0;
    return a2 / (PI * denom * denom);
}

// Schlick fresnel function
vec3 schlickFresnel(float VoH, vec3 f0)
{
    return f0 + (1.0 - f0) * pow(1.0 - VoH, 5.0);
}

// Schlick-GGX geometry function
float schlick_ggx(float NoL, float NoV, float roughness)
{
    float k = roughness + 1.0;
    k *= k * 0.125;
    float gl = NoL / (NoL * (1.0 - k) + k);
    float gv = NoV / (NoV * (1.0 - k) + k);
    return gl * gv;
}

// Evaluate the Cook-Torrance specular BRDF
vec3 cookTorranceBRDF(float NoL, float NoV, float NoH, float VoH, vec3 F, float roughness)
{
    vec3 DFG = ggx(NoH, roughness) * F * schlick_ggx(NoL, NoV, roughness);
    float denom = 4.0 * NoL * NoV + 0.0001;
    return DFG / denom;
}

// Evaluate combined diffuse and specular BRDF
vec3 evalBRDF(vec3 n, vec3 v, vec3 l, Material m)
{
    // Common dot products
    float NoV = saturate(dot(n, v));
    float NoL = saturate(dot(n, l));
    vec3 h = normalize(v + l);
    float NoH = saturate(dot(n, h));
    float VoH = saturate(dot(v, h));

    // Use standard approximation of default fresnel
    vec3 f0 = mix(vec3(0.04), m.albedo, m.metalness);
    vec3 F = schlickFresnel(VoH, f0);

    // Diffuse amount
    vec3 Kd = (1.0 - F) * (1.0 - m.metalness);

    return (Kd * lambertBRFD(m.albedo) + cookTorranceBRDF(NoL, NoV, NoH, VoH, F, m.roughness)) * NoL;
}

// --INTERSECTION-------------------------------------------------------------------
float iSphere(Ray r, int i)
{
    vec4 s = SPHERES[i];
    vec3 L = s.xyz- r.o;
    float tc = dot(L, r.d);
    float d2 = dot(L, L) - tc * tc;
    float r2 = s.w * s.w;
    if (d2 > r2)
        return r.t;

    float tlc = sqrt(r2 - d2);
    float t0 = tc - tlc;
    float t1 = tc + tlc;
    if (t0 > t1) {
        float tmp = t0;
        t0 = t1;
        t1 = tmp;
    }
    if (t0 < 0.0) {
        if (t1 < 0.0)
            return r.t;
        return t1;
    }
    return t0;
}

// --TRACING-------------------------------------------------------------------
Hit traceRay(Ray r)
{
    int object = -1;
    float t = r.t;
    for (int i = 0; i < NUM_SPHERES; ++i) {
        float nt = iSphere(r, i);
        if (nt < t) {
            t = nt;
            object = i;
        }
    }
    vec3 position = vec3(0);
    vec3 normal = vec3(0);
    if (object >= 0) {
        position = r.o + t * r.d;
        normal = normalize(position - SPHERES[object].xyz);
    }
    return Hit(object >= 0, position, normal, evalMaterial(position, object));
}

vec3 tracePath(vec2 px)
{
    vec3 ei = vec3(0);
    for (int j = 0; j < SAMPLES_PER_PIXEL; ++j) {
        // Generate ray
        vec2 sample_px = gl_FragCoord.xy + vec2(rnd(), rnd());
        Ray r = Ray(vec3(0,0,-15.5), getViewRay(sample_px, 45.0), 100.0);

        int bounce = 1;
        vec3 throughput = vec3(1);
        while (true) {
            // Fire away!
            Hit hit = traceRay(r);

            // Cut ray on miss, "backface" hit or being outside the box
            if (!hit.hit || dot(hit.normal, r.d) > 0.0 || hit.position.z < -5.0)
                break;

            // Collect common info
            Material m = hit.material;
            vec3 n = hit.normal;
            vec3 p = hit.position + hit.normal * EPSILON;

            // Add hacky emission on first hit to draw lights
            if (bounce == 1)
                ei += throughput * m.emission;

            // Sample lights
            for (int i = 0; i < NUM_LIGHTS; ++i) {
                // Generate point on light surface
                vec4 ls = sampleLight(i);
                vec3 pL = ls.xyz;
                float pdf = ls.w;

                // Generate shadow ray
                vec3 toL = pL - p;
                Ray sr;
                sr.o = p;
                sr.t = length(toL);
                sr.d = toL / sr.t;

                // Test visibility
                Hit sh = traceRay(sr);
                if (!sh.hit) {
                    // Add light contribution when visible
                    float r2 = sr.t * sr.t;
                    vec3 lN = vec3(0, -1, 0); // TODO: generic
                    if (dot(lN, -sr.d) > 0.0) {
                    	vec3 E = LIGHTS[i].E;
                    	ei += throughput * evalBRDF(hit.normal, -r.d, sr.d, m) * E / (r2 * pdf);
                    }
                }
            }

            // Russian roulette for termination
            if (bounce >= BOUNCES && rnd() < P_TERMINATE)
                break;

            // Get random direction for reflection ray
            vec3 rd = cosineSampleHemisphere();
            // Rotate by normal frame
            rd = normalize(formBasis(n) * rd);
            float pdf = cosineHemispherePDF(dot(n, rd));
            // TODO: Multiple importance sampling on diffuse and specular?
            throughput *= evalBRDF(hit.normal, -r.d, rd, m) / pdf;
            r.d = rd;
            r.o = p;
            bounce++;
        }
    }
    return ei / float(SAMPLES_PER_PIXEL);
}


void main()
{
    // Reseed by iq
    seed = iTime + fragCoord.y * fragCoord.x / iResolution.x + fragCoord.y / iResolution.y;
    vec3 color = tracePath(gl_FragCoord.xy);
    fragColor = vec4(ltos3(color.x, color.y, color.z), 1);
}