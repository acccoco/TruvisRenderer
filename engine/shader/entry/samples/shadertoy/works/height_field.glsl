// Rays traverse a uniform grid and are tested against a triangle (actually this is just
// a ray-versus-plane test). Rather than sampling the heightfield 3 times to get the triangle
// vertices, previous vertices are kept and only one of the vertices is updated at each step.
// So the heightfield is only sampled once per step.
//
// The algorithm is similar to triangle strip rasterisation, in that each new vertex
// together with previous 2 vertices define a triangle.
//

//
// The shaders of this series:
//
//   Triangulated Heightfield Trick 1 - https://www.shadertoy.com/view/XlcBRX (Rigid, right-triangle)
//   Triangulated Heightfield Trick 2 - https://www.shadertoy.com/view/tlXSzB (Rigid, equilateral)
//   Triangulated Heightfield Trick 3 - https://www.shadertoy.com/view/ttsSzX (Deforming, equilateral)
//   Tetrahedral Voxel Traversal      - https://www.shadertoy.com/view/wtfXWB (Rigid, tetrahedron)
//


// Use this to toggle between taking 1 sample of the heightfield and taking
// 3 samples (to fully construct the triangle on every step).
#define SINGLE_SAMPLE 1

float minh = 0.0, maxh = 6.0;
vec3 nn = vec3(0);

float hash(float n)
{
    return fract(sin(n) * 43758.5453);
}

float noise(vec3 p)
{
    return hash(p.x + p.y*57.0 + p.z*117.0);
}

float valnoise(vec3 p)
{
    vec3 c = floor(p);
    vec3 f = smoothstep(0., 1., fract(p));
    return mix(
        mix (mix(noise(c + vec3(0, 0, 0)), noise(c + vec3(1, 0, 0)), f.x),
             mix(noise(c + vec3(0, 1, 0)), noise(c + vec3(1, 1, 0)), f.x), f.y),
        mix (mix(noise(c + vec3(0, 0, 1)), noise(c + vec3(1, 0, 1)), f.x),
             mix(noise(c + vec3(0, 1, 1)), noise(c + vec3(1, 1, 1)), f.x), f.y),
        f.z);
}

float fbm(vec3 p)
{
    float f = 0.;
    for(int i = 0; i < 5; ++i)
        f += (valnoise(p * exp2(float(i))) - .5) / exp2(float(i));
    return f;
}

float height(vec2 p)
{
    float h = mix(minh, maxh * 1.3, pow(clamp(.2 + .8 * fbm(vec3(p / 6., 0.)), 0., 1.), 1.3));
    h += valnoise(vec3(p, .3));
    return h;
}

// The raytracing function
vec3 tr2(vec3 o,vec3 r)
{
    // Start ray at upper Y bounds
    if(o.y > maxh)
        o += r * (maxh - o.y) / r.y;

    vec2 oc = vec2(floor(o.x), floor(o.z)), c;
    vec2 dn = normalize(vec2(-1, 1));
    vec3 ta, tb, tc;

    // Initialise the triangle vertices
    ta = vec3(oc.x, height(oc + vec2(0, 0)), oc.y);
    tc = vec3(oc.x + 1., height(oc + vec2(1, 1)), oc.y + 1.);
    if(fract(o.z) < fract(o.x))
        tb = vec3(oc.x + 1., height(oc + vec2(1, 0)), oc.y + 0.);
    else
        tb = vec3(oc.x, height(oc + vec2(0, 1)), oc.y + 1.);

    float t0 = 1e-4, t1;

    // Ray slopes
    vec2 dd = vec2(1) / r.xz;
    float dnt = 1.0 / dot(r.xz, dn);

    float s = max(sign(dnt), 0.);
    c = ((oc + max(sign(r.xz), 0.)) - o.xz) * dd;

    vec3 rs = sign(r);

    for(int i = 0; i < 450; ++i)
    {
        t1 = min(c.x, c.y);

        // Test ray against diagonal plane
        float dt = dot(oc - o.xz, dn) * dnt;
        if(dt > t0 && dt < t1)
            t1 = dt;

#if !SINGLE_SAMPLE
        // Sample the heightfield for all three vertices.
        vec2 of = (dot(o.xz + r.xz * (t0 + t1) * .5 - oc, dn) > 0.) ? vec2(0, 1) : vec2(1, 0);
        tb = vec3(oc.x + of.x, height(oc + of), oc.y + of.y);
        ta = vec3(oc.x, height(oc + vec2(0, 0)), oc.y);
        tc = vec3(oc.x + 1., height(oc + vec2(1, 1)), oc.y + 1.);
#endif

        // Test ray against triangle plane
        vec3 hn = cross(ta - tb, tc - tb);
        float hh = dot(ta - o, hn) / dot(r, hn);

        if(hh > t0 && hh < t1)
        {
            // Intersection with triangle has been found
            nn = hn;
            return o + r * hh;
        }

#if SINGLE_SAMPLE
        vec2 offset;

        // Get an "axis selector", which has 1.0 for the near (intersected) axis
        // and 0.0 for the far one
        vec2 ss = step(c, c.yx);

        // Get the coordinate offset of where to read the next vertex height from
        if(dt >= t0 && dt < c.x && dt < c.y)
        {
            offset = vec2(1. - s, s);
        }
        else
        {
            offset = dot(r.xz, ss) > 0. ? vec2(2, 1) : vec2(-1, 0);

            if(c.y < c.x)
                offset = offset.yx;
        }

        // Get the next vertex
        vec3 tnew = vec3(oc + offset, height(oc + offset)).xzy;

        // Update the triangle vertices.
        if(dt >= t0 && dt < c.x && dt < c.y)
        {
            tb = tnew;
        }
        else
        {
            // Swap vertex order based on sign of ray axis
            if(dot(r.xz, ss) > 0.)
            {
                ta = tb;
                tb = tc;
                tc = tnew;
            }
            else
            {
                tc = tb;
                tb = ta;
                ta = tnew;
            }

            // Step the grid coordinates along to the next cell
            oc.xy += rs.xz * ss;
            c.xy += dd.xy * rs.xz * ss;
        }
#else
        // Get an "axis selector", which has 1.0 for the near (intersected) axis
        // and 0.0 for the far one
        vec2 ss = step(c, c.yx);

        if(dt < t0 || dt >= c.x || dt >= c.y)
        {
            // Step the grid coordinates along to the next cell
            oc.xy += rs.xz * ss;
            c.xy += dd.xy * rs.xz * ss;
        }

#endif
        t0 = t1;

        // Test if the ray left the upper Y bounds
        if(((maxh - o.y) / r.y < t0 && r.y > 0.) || t0 > 200.)
            return vec3(10000);

    }
    return vec3(10000);
}

// Ray direction function
vec3 rfunc(vec2 uv)
{
    vec3 r = normalize(vec3(uv.xy, -1.3));
    float ang = .7;
    r.yz *= mat2(cos(ang), sin(ang), -sin(ang), cos(ang));
    return r;
}

float chequer(vec2 p)
{
    return step(0.5, fract(p.x + step(0.5, fract(p.y)) * 0.5));
}

void main()
{
    vec2 uv = fragCoord / iResolution.xy;

    vec2 t = uv * 2. - 1. + 1e-3;
    t.x *= iResolution.x / iResolution.y;

    // Setup primary ray
    vec3 o = vec3(1.4, 9.5, -iTime), r = rfunc(t);

    // Trace primary ray
    vec3 rp = tr2(o, r);

    // Surface normal
    vec3 n = normalize(nn);
    if(n.y < 0.)
        n =- n;

    // Checkerboard pattern
    vec3 col = vec3(mix(.8, 1., chequer(rp.xz / 2.)));

    if(fract(rp.z) < fract(rp.x))
		col *= .7;

    // Light direction
    vec3 ld = normalize(vec3(1.5, 1, -2));

    // Directional shadow (raytraced)
    vec3 rp2 = tr2(rp + n*1e-4 + ld * 1e-4, ld);
    if(distance(rp, rp2) < 1000.)
        col *= .4 * vec3(.65, .65, 1);

    // Basic colouration
    col *= mix(vec3(1, .8, .5) / 2., vec3(.3, 1, .3) / 4., 1. - clamp(rp.y / 2., 0., 1.));
    col = mix(col, vec3(1) * .7, pow(clamp((rp.y - 2.5) / 2., 0., 1.), 2.));

    // Directional light falloff
    col *= pow(.5 + .5 * dot(n, ld), 1.);

    // Fog
    col = mix(vec3(.65, .65, 1), col, exp2(-distance(rp, o) / 1024.));

    // Clamp and gamma-correct
    fragColor.rgb = pow(clamp(col * 2., 0., 1.), vec3(1. / 2.2));
}


