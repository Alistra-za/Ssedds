// Repo-owned stand-in for the Community Shaders Lighting.hlsl (written for sdc's render3d tests; not derived
// from CS). It declares the cbuffer, texture and sampler names render3d binds by name, so the whole render path
// (permutation defines, compile, reflection, cbuffer writes, default textures, draw, readback) runs on WARP
// without the user's CS install. GPL-3.0-or-later, like the rest of sdc.
//
// Failure hook for the tests: the pixel permutation without DO_ALPHA_TEST does not compile, so a fixture with
// one such shape gets exactly one compile error while the others still draw. A test that passes the feature
// define STANDIN_OPAQUE_OK lets those permutations compile (opaque shapes for the alpha check).
//
// Pixel hooks for the P1 tests (feature defines):
// - VC (render3d's vertex-colour define, VS only): the VS reads COLOR0 and passes it on; otherwise white.
// - STANDIN_FLAT: the PS returns 0.25 x diffuse x vertex colour + EmitColor, plus 0.5 green when PBRFlags is non-zero, plus an unread t6 emissive map (see EmissiveMap), with no
//   lighting, so cbuffer edits (emissive, PBR flags) and texture edits show in pixels without saturating.
// - STANDIN_TBN: the PS returns N.L only, N being the normal map turned into the vertex tangent frame. The frame
//   is read from the SSE vertex record the way render3d packs it (mesh.rs pack_tbn): row 0 =
//   (POSITION.w, NORMAL.w, BINORMAL.w) (the file's Bitangent), row 1 = BINORMAL.xyz (the file's Tangent),
//   row 2 = NORMAL.xyz; the map's x, y, z weight rows 0, 1, 2.
// - Point lights: the default (lit) branch reads CS's vanilla point-light fields (PointLightPosition[7] with the
//   radius in w, PointLightColor[7], NumLightNumShadowLight.x) and adds base x colour x (1 - (d/r)^2) x N.L per
//   light, camera-relative like World. Zero lights add exactly nothing.
// - STANDIN_FEATURE_DATA (with STANDIN_FLAT): the flat colour is scaled by CS's FeatureData
//   linearLightingSettings.ambientMult, so a settings change that only rewrites the static cbuffer block shows in
//   pixels (P1-C settings tests).
// - STANDIN_SKIN (with STANDIN_FLAT): CS's SkinPerGeometry (b7) and skin extra map (t71) are declared, and the flat
//   colour adds skinMaterialFlags.x x the t71 RGB, so the RFAOS lookup shows in pixels (P1-C Task 8 tests).
// - STANDIN_REPLAY (with STANDIN_FLAT): the flat colour adds DirLightColor (PS b2), so a replay reads every PS
//   cbuffer and a zero-padded short b2 adds nothing (v2 D2 tests).
// - DEFERRED (render3d's deferred define): main writes CS's 8 render targets (PsOut, CS's field names). Diffuse is
//   the forward colour; RT1-7 hold fixed markers (v2 D3 tests). It declares the names the debug block reads, but its
//   `return psout ;` has a space, so this file has no debug anchor; tests/deferred.rs writes a copy without that
//   space to get one (the temp-copy pattern of tests/settings.rs).
// - STANDIN_FORWARD_AUX (forward permutations only): main also writes SV_Target1/2 like CS's forward PS_OUTPUT, so a
//   test sees a forward draw that wrongly gets RT1-7 bound.
// - STANDIN_FORWARD_BROKEN: every forward pixel permutation fails to compile; DEFERRED ones still compile.
// - STANDIN_DEFERRED_TINT (DEFERRED permutations only): Diffuse gets +0.5 green, so a DEFERRED program drawn into a
//   forward target shows (v2 D4 review).
// - DYNAMIC_CUBEMAPS (with STANDIN_FLAT, plain forward main only): the colour adds the level-15 +Z sample of
//   EnvReflectionsTexture at t30, CS's forward DC register, so the env cube binding shows in pixels (v2 D5). With
//   STANDIN_ENV_T31 it samples EnvTexture at t31 instead (CS's other forward DC register).
// - STANDIN_TIMER (with STANDIN_FLAT): the PS declares CS's Timer (b2) and the flat colour adds 0.5 x frac(Timer)
//   to red, so the clock's time shows in pixels (v2 B3).

struct VsOut
{
    float4 Position : SV_Position;
    float2 Uv : TEXCOORD0;
    float3 Normal : TEXCOORD1;
    float3 Row0 : TEXCOORD2;
    float3 Row1 : TEXCOORD3;
    float4 Color : TEXCOORD4;
    float3 WorldPosition : TEXCOORD5;
};

#if defined(VSHADER)

cbuffer StandinGeometry : register(b2)
{
    row_major float3x4 World;
    row_major float4x4 ViewProj;
    float4 EyePosition;
};

cbuffer StandinMaterial : register(b1)
{
    float4 TexcoordOffset;
};

struct VsIn
{
    float4 Position : POSITION0;
    float2 Uv : TEXCOORD0;
    float4 Normal : NORMAL0;
    float4 Binormal : BINORMAL0;
#if defined(VC)
    float4 Color : COLOR0;
#endif
};

VsOut main(VsIn v)
{
    VsOut o;
    float3 p = mul(World, float4(v.Position.xyz, 1.0)) - EyePosition.xyz;
    o.Position = mul(ViewProj, float4(p, 1.0));
    o.WorldPosition = p;
    o.Uv = v.Uv * TexcoordOffset.zw + TexcoordOffset.xy;
    o.Normal = mul((float3x3)World, v.Normal.xyz * 2.0 - 1.0);
    o.Row0 = mul((float3x3)World, float3(v.Position.w, v.Normal.w * 2.0 - 1.0, v.Binormal.w * 2.0 - 1.0));
    o.Row1 = mul((float3x3)World, v.Binormal.xyz * 2.0 - 1.0);
#if defined(VC)
    o.Color = v.Color;
#else
    o.Color = float4(1.0, 1.0, 1.0, 1.0);
#endif
    return o;
}

#elif defined(PSHADER)

#if !defined(DO_ALPHA_TEST) && !defined(STANDIN_OPAQUE_OK)
#error stand-in: this permutation is meant to fail (test fixture)
#endif
#if defined(STANDIN_FORWARD_BROKEN) && !defined(DEFERRED)
#error stand-in: forward pixel permutations fail (test fixture)
#endif

cbuffer StandinLight : register(b2)
{
    float4 DirLightDirection;
    float4 DirLightColor;
    row_major float3x4 DirectionalAmbient;
    float4 PointLightPosition[7];
    float4 PointLightColor[7];
    float2 NumLightNumShadowLight;
#if defined(STANDIN_TIMER)
    float Timer;
#endif
};

cbuffer StandinMaterialPs : register(b1)
{
    float4 EmitColor;
    uint PBRFlags;
};

Texture2D<float4> DiffuseMap : register(t0);
Texture2D<float4> NormalMap : register(t1);
// t6 is where render3d binds a TruePBR emissive map (slot 2). STANDIN_FLAT reads it behind a test that is never true
// (EmitColor.w is never written), so the register is reflected and bindable but no pixel changes: the hot-swap tests
// see HasEmissive flip through the PBRFlags green channel.
Texture2D<float4> EmissiveMap : register(t6);
SamplerState DiffuseSampler : register(s0);

#if defined(STANDIN_FEATURE_DATA)
struct StandinLinearLightingSettings
{
    float ambientMult;
    float3 pad;
};

cbuffer FeatureData : register(b6)
{
    StandinLinearLightingSettings linearLightingSettings;
};
#endif

#if defined(STANDIN_SKIN)
cbuffer SkinPerGeometry : register(b7)
{
    float4 skinPerGeometry;
    float4 skinMaterialFlags;
};
Texture2D<float4> TexSkinExtraSampler : register(t71);
#endif

float4 shade(VsOut i)
{
    float4 base = DiffuseMap.Sample(DiffuseSampler, i.Uv);
    float3 bump = NormalMap.Sample(DiffuseSampler, i.Uv).xyz * 2.0 - 1.0;
#if defined(STANDIN_TBN)
    float3 nt = normalize(bump.x * i.Row0 + bump.y * i.Row1 + bump.z * i.Normal);
    return float4(saturate(dot(nt, DirLightDirection.xyz)).xxx, 1.0);
#elif defined(STANDIN_FLAT)
    float flagGreen = (PBRFlags & 0xFFFu) != 0 ? 0.5 : 0.0;
    float3 emissive = EmitColor.a < -1.0 ? EmissiveMap.SampleLevel(DiffuseSampler, i.Uv, 0).rgb : float3(0.0, 0.0, 0.0);
    float3 col = 0.25 * base.rgb * i.Color.rgb + EmitColor.rgb + emissive + float3(0.0, flagGreen, 0.0);
#if defined(STANDIN_FEATURE_DATA)
    col *= linearLightingSettings.ambientMult;
#endif
#if defined(STANDIN_SKIN)
    col += skinMaterialFlags.x * TexSkinExtraSampler.SampleLevel(DiffuseSampler, i.Uv, 0).rgb;
#endif
#if defined(STANDIN_REPLAY)
    col += DirLightColor.rgb;
#endif
#if defined(STANDIN_TIMER)
    col.r += 0.5 * frac(Timer);
#endif
    return float4(col, base.a * 0.5);
#else
    float3 n = normalize(i.Normal + 0.001 * bump);
    float ndl = saturate(dot(n, DirLightDirection.xyz));
    float3 ambient = mul(DirectionalAmbient, float4(n, 1.0));
    // A floor of half the base colour keeps every covered pixel visible whatever the normals are.
    float3 c = base.rgb * (0.5 + 0.5 * ndl * DirLightColor.rgb + ambient);
    uint numLights = min(7, uint(NumLightNumShadowLight.x));
    for (uint k = 0; k < numLights; k++)
    {
        float3 toLight = PointLightPosition[k].xyz - i.WorldPosition;
        float f = saturate(length(toLight) / PointLightPosition[k].w);
        c += base.rgb * PointLightColor[k].rgb * (1.0 - f * f) * saturate(dot(n, normalize(toLight)));
    }
    // Alpha below 1 on purpose: the display readback must still be opaque.
    return float4(c, base.a * 0.5);
#endif
}

#if defined(DEFERRED)
struct MaterialProperties
{
    float3 BaseColor;
    float Roughness;
    float Metallic;
    float AO;
};

struct PsOut
{
    float4 Diffuse : SV_Target0;
    float4 MotionVectors : SV_Target1;
    float4 NormalGlossiness : SV_Target2;
    float4 Albedo : SV_Target3;
    float4 Specular : SV_Target4;
    float4 Reflectance : SV_Target5;
    float4 Masks : SV_Target6;
    float4 Masks2 : SV_Target7;
};

PsOut main(VsOut i)
{
    PsOut psout;
    MaterialProperties material = (MaterialProperties)0;
    material.BaseColor = DiffuseMap.Sample(DiffuseSampler, i.Uv).rgb;
    float3 worldNormal = normalize(i.Normal);
    float glossiness = 0.25;
    float vertexAO = 0.5;
    psout.Diffuse = shade(i);
#    if defined(STANDIN_DEFERRED_TINT)
    psout.Diffuse.g += 0.5;
#    endif
    // Markers: exact in each RT's format and different per RT, so a swapped RTV shows.
    psout.MotionVectors = float4(0.25, 0.5, 0.0, 0.0);
    psout.NormalGlossiness = float4(1.0, 0.0, 1.0, 1.0);
    psout.Albedo = float4(0.0, 1.0, 0.0, 0.0);
    psout.Specular = float4(0.5, 0.25, 0.125, 0.0);
    psout.Reflectance = float4(0.25, 0.125, 0.5, 0.0);
    psout.Masks = float4(2.0, 4.0, 0.0625, 0.0);
    psout.Masks2 = float4(1.0, 0.0, 0.0, 0.0);
    return psout ;
}
#elif defined(STANDIN_FORWARD_AUX)
// Like CS's forward PS_OUTPUT: SV_Target1/2 written too, with conspicuous values (alpha 1, so a blend replaces).
struct PsOutForward
{
    float4 Diffuse : SV_Target0;
    float4 MotionVectors : SV_Target1;
    float4 NormalGlossiness : SV_Target2;
};

PsOutForward main(VsOut i)
{
    PsOutForward o;
    o.Diffuse = shade(i);
    o.MotionVectors = float4(0.75, 0.125, 0.0, 1.0);
    o.NormalGlossiness = float4(0.0, 1.0, 0.0, 1.0);
    return o;
}
#else
#if defined(DYNAMIC_CUBEMAPS) && defined(STANDIN_FLAT)
// CS's forward DC path samples DynamicCubemaps::EnvReflectionsTexture at t30 and EnvTexture at t31
// (DynamicCubemaps.hlsli:17-18).
#	if defined(STANDIN_ENV_T31)
TextureCube<float3> EnvTexture : register(t31);
#		define STANDIN_ENV EnvTexture
#	else
TextureCube<float3> EnvReflectionsTexture : register(t30);
#		define STANDIN_ENV EnvReflectionsTexture
#	endif
#endif
float4 main(VsOut i) : SV_Target
{
    float4 c = shade(i);
#if defined(DYNAMIC_CUBEMAPS) && defined(STANDIN_FLAT)
    c.rgb += STANDIN_ENV.SampleLevel(DiffuseSampler, float3(0.0, 0.0, 1.0), 15);
#endif
    return c;
}
#endif

#endif
