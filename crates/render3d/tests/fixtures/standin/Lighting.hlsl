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
// - STANDIN_FLAT: the PS returns 0.25 x diffuse x vertex colour + EmitColor, plus 0.5 green when PBRFlags is non-zero, with no
//   lighting, so cbuffer edits (emissive, PBR flags) and texture edits show in pixels without saturating.
// - STANDIN_TBN: the PS returns N.L only, N being the normal map turned into the vertex tangent frame. The frame
//   is read from the SSE vertex record the way render3d packs it (mesh.rs pack_tbn): row 0 =
//   (POSITION.w, NORMAL.w, BINORMAL.w) (the file's Bitangent), row 1 = BINORMAL.xyz (the file's Tangent),
//   row 2 = NORMAL.xyz; the map's x, y, z weight rows 0, 1, 2.

struct VsOut
{
    float4 Position : SV_Position;
    float2 Uv : TEXCOORD0;
    float3 Normal : TEXCOORD1;
    float3 Row0 : TEXCOORD2;
    float3 Row1 : TEXCOORD3;
    float4 Color : TEXCOORD4;
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

cbuffer StandinLight : register(b2)
{
    float4 DirLightDirection;
    float4 DirLightColor;
    row_major float3x4 DirectionalAmbient;
};

cbuffer StandinMaterialPs : register(b1)
{
    float4 EmitColor;
    uint PBRFlags;
};

Texture2D<float4> DiffuseMap : register(t0);
Texture2D<float4> NormalMap : register(t1);
SamplerState DiffuseSampler : register(s0);

float4 main(VsOut i) : SV_Target
{
    float4 base = DiffuseMap.Sample(DiffuseSampler, i.Uv);
    float3 bump = NormalMap.Sample(DiffuseSampler, i.Uv).xyz * 2.0 - 1.0;
#if defined(STANDIN_TBN)
    float3 nt = normalize(bump.x * i.Row0 + bump.y * i.Row1 + bump.z * i.Normal);
    return float4(saturate(dot(nt, DirLightDirection.xyz)).xxx, 1.0);
#elif defined(STANDIN_FLAT)
    float flagGreen = (PBRFlags & 0xFFFu) != 0 ? 0.5 : 0.0;
    return float4(0.25 * base.rgb * i.Color.rgb + EmitColor.rgb + float3(0.0, flagGreen, 0.0), base.a * 0.5);
#else
    float3 n = normalize(i.Normal + 0.001 * bump);
    float ndl = saturate(dot(n, DirLightDirection.xyz));
    float3 ambient = mul(DirectionalAmbient, float4(n, 1.0));
    // A floor of half the base colour keeps every covered pixel visible whatever the normals are.
    float3 c = base.rgb * (0.5 + 0.5 * ndl * DirLightColor.rgb + ambient);
    // Alpha below 1 on purpose: the display readback must still be opaque.
    return float4(c, base.a * 0.5);
#endif
}

#endif
