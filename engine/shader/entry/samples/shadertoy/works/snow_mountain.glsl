// Created by David Lovera - Unix/2015
// License Creative Commons Attribution-NonCommercial-ShareAlike 3.0 Unported License.
// mainly based on Kali great work...

#define RAY_STEPS 100
#define SHADOW_STEPS 50
#define LIGHT_COLOR vec3(1.,.97,.93)
#define AMBIENT_COLOR vec3(.2,.5,1.0)

#define SPECULAR 0.65
#define DIFFUSE  2.0
#define AMBIENT  0.55

#define BRIGHTNESS 2.5
#define GAMMA 1.45
#define SATURATION .8

#define detail .00004
#define t 2.0+iTime*.2

vec3  lightdir=normalize(vec3(1.1,-0.55,-1.));
const vec3 origin=vec3(-1.,0.2,0.);
float det=0.0;
vec3 pth1;

mat2 rot(float a) {
	return mat2(cos(a),sin(a),-sin(a),cos(a));
}

vec4 formula(vec4 p) {
		p.xz = abs(p.xz+1.5+sin(iTime*0.1*p.z)*0.1)-abs(p.xz-1.5+sin(iTime*0.01*p.z)*0.1)-p.xz;
		p=p*2./clamp(dot(p.xz,p.xz),.15,1.)-vec4(0.5,0.5,0.8,0.);
		p.xy*=rot(.5);
	    return p;
}

vec4 formula2(vec4 p) {
		p.xz = abs(p.xz+1.5+cos(iTime*0.1+p.z)*0.1)-abs(p.xz-1.5+sin(iTime*0.1+p.z)*0.1)-p.xz;
		p=p*2./clamp(dot(p.xy,p.xy),.15,1.)-vec4(0.5,0.5,0.8,0.);
		p.xy*=rot(.5);
	    return p;
}

float screen(vec3 p) {
	float d1=length(p.yz-vec2(1.25,0.))-.1;
	float d2=0.;
	return min(max(d1,abs(p.x-.3)-.01),max(d2,abs(p.x+20.3)-.01));
}

vec2 de(vec3 pos) {
	float hid=0.;
	vec3 tpos=pos;
	tpos.z=abs(4.-mod(tpos.z,8.));
	vec4 p=vec4(tpos,1.5);
	float y=max(0.,.35-abs(pos.y-3.35))/.35;

	// add mountain
	for (int i=0; i<6; i++) {p=formula(p);}
	float fr=max(-tpos.x-4.,(length(max(vec2(0.),p.yz-2.))-.5)/p.w);

	p=vec4(tpos,1.5);
	y=max(0.,.35-abs(pos.y-3.35))/.35;

	// add snow
	for (int i=0; i<8; i++) {p=formula2(p);}
	float fr2=max(-tpos.x-4.,(length(max(vec2(0.),p.yz-3.)))/p.w);

	float sc=fr2;
	float d=min(sc,fr);
	if (abs(d-sc)<.001) hid=1.;
	return vec2(d,hid);
}

vec2 colorize(vec3 p) {
	p.z=abs(2.-mod(p.z,4.));
	float es, l=es=0.;
	float ot=1000.;
	for (int i = 0; i < 15; i++) {
		p=formula(vec4(p,0.)).xyz;
				float pl = l;
				l = length(p);
				es+= exp(-10. / abs(l - pl));
				ot=min(ot,abs(l-3.));
	}
	return vec2(es,ot);
}

vec3 path(float ti) {
	vec3  p=vec3(-sin(ti*2.)*.1,(1.-sin(ti*.6))*.5,-cos(ti*.25)*30.)*.2;
	return p;
}

vec3 normal(vec3 p) {
	vec3 e = vec3(0.0,det,0.0);
	return normalize(vec3(
			de(p+e.yxx).x-de(p-e.yxx).x,
			de(p+e.xyx).x-de(p-e.xyx).x,
			de(p+e.xxy).x-de(p-e.xxy).x
			)
		);
}

float shadow(vec3 pos, vec3 sdir) {
	float sh=1.0;
	float totdist =2.0*det;
	float dist=10.;
		for (int steps=0; steps<SHADOW_STEPS; steps++) {
			if (totdist<1. && dist>detail) {
				vec3 p = pos - totdist * sdir;
				dist = de(p).x;
				sh = min( sh, max(50.*dist/totdist,0.0) );
				totdist += max(.01,dist);
			}
		}

    return clamp(sh,0.1,1.0);
}

float calcAO( const vec3 pos, const vec3 nor ) {
	float aodet=detail*40.;
	float totao = 0.0;
    float sca = 13.0;
    for( int aoi=0; aoi<5; aoi++ ) {
        float hr = aodet*float(aoi*aoi);
        vec3 aopos =  nor * hr + pos;
        float dd = de( aopos ).x;
        totao += -(dd-hr)*sca;
        sca *= 0.1;
    }
    return clamp( 1.0 - 5.0*totao, 0., 1.0 );
}

vec3 light(in vec3 p, in vec3 dir, in vec3 n, in float hid) {
	float sh=shadow(p, lightdir);
	float ao=calcAO(p,n);
	float diff=max(0.,dot(lightdir,-n))*sh*DIFFUSE;
	vec3 amb=max(.5,dot(dir,-n))*AMBIENT*AMBIENT_COLOR;
	vec3 r = reflect(lightdir,n);
	float spec=pow(max(0.,dot(dir,-r))*sh,15.)*SPECULAR;
	vec3 col;
	vec2 getcol=colorize(p);
	if (hid>.5) {col=vec3(1.); spec=spec*spec;}
	else{
		float k=pow(getcol.x*.01,2.);
		col=mix(vec3(k,k*k,k*k),vec3(k),.5)+.1;
		col+=pow(max(0.,1.-getcol.y),5.)*.3;
	}
	col=col*ao*(amb+diff*LIGHT_COLOR)+spec*LIGHT_COLOR;
	return col;
}

vec3 raymarch(in vec3 from, in vec3 dir)

{
	float glow,eglow,totdist=glow=0.;
	vec2 d=vec2(1.,0.);
	vec3 p, col=vec3(0.);

	for (int i=0; i<RAY_STEPS; i++) {
		if (d.x>det && totdist<30.0) {
			p=from+totdist*dir;
			d=de(p);
			det=detail*(1.+totdist*50.);
			totdist+=d.x;
			if(d.x<0.015) glow+=max(0.,.015-d.x)*exp(-totdist);
		}
	}

	float l=max(0.,dot(normalize(-dir),normalize(lightdir)));
	vec3 backg=vec3(max(0.,1.))*AMBIENT_COLOR*.5*max(0.4,l);

	if (d.x<det || totdist<3.) {
		p=p-abs(d.x-det)*dir;
		vec3 norm=normal(p);
		col=light(p, dir, norm, d.y);
		col = mix(col, backg, 1.0-exp(-.15*pow(totdist,1.5)));
	} else {
		col=backg;
	    vec3 st = (dir * 3.+ vec3(1.3,2.5,1.25)) * 1.3;
		for (int i = 0; i < 7; i++) st = abs(st) / dot(st,st) - .9;
		col+= min( 1., pow( min( 5., length(st) ), 3. ) * .0025 );
	}

	vec3 lglow=LIGHT_COLOR*pow(l,25.)*.5;
	col+=glow*(.5+l*.5)*LIGHT_COLOR*.7;
	col+=lglow*exp(min(30.,totdist)*.02);
	return col;
}

vec3 move(inout vec3 dir) {
	vec3 go=path(t);
	vec3 adv=path(t+.7);
	float hd=de(adv).x;
	vec3 advec=normalize(adv-go);
	float an=adv.x-go.x; an*=min(1.,abs(adv.z-go.z))*sign(adv.z-go.z)*.7;
	dir.xy*=mat2(cos(an),sin(an),-sin(an),cos(an));
    an=advec.y*1.7;
	dir.yz*=mat2(cos(an),sin(an),-sin(an),cos(an));
	an=atan(advec.x,advec.z);
	dir.xz*=mat2(cos(an),sin(an),-sin(an),cos(an));
	return go;
}

void main()
{

   vec2 q = fragCoord.xy / iResolution.xy;
   if (q.y < .12 || q.y >= .88) {
		fragColor=vec4(0.,0.,0.,1.);
		return;
	}

    lightdir.y = lightdir.y + iTime*0.01;

	pth1 = path(t+.3)+origin;
	vec2 uv = fragCoord.xy / iResolution.xy*2.-1.;
	uv.y*=iResolution.y/iResolution.x;
	vec3 dir=normalize(vec3(uv*.8,1.));
	vec3 from=origin+move(dir);
	vec3 color=raymarch(from,dir);
	color=clamp(color,vec3(.0),vec3(1.));
	color=pow(color,vec3(GAMMA))*BRIGHTNESS;
	color=mix(vec3(length(color)),color,SATURATION);
	fragColor = vec4(color,1.);
}