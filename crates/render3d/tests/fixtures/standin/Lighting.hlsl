// Repo-owned stand-in for the Community Shaders Lighting.hlsl (written for sdc's render3d tests; not derived
// from CS). It declares the cbuffer, texture and sampler names render3d binds by name, so the whole render path
// (permutation defines, compile, reflection, cbuffer writes, default textures, draw, readback) runs on WARP
// without the user's CS install. GPL-3.0-or-later, like the rest of sdc.
//
// Failure hook for the tests: the pixel permutation without DO_ALPHA_TEST does not compile, so a fixture with
// one such shape gets exactly one compile error while the others still draw. A test that passes the feature
// define STANDIN_OPAQUE_OK lets those permutations compile (opaque shapes for the alpha check).

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
};

struct VsOut
{
    float4 Position : SV_Position;
    float2 Uv : TEXCOORD0;
    float3 Normal : TEXCOORD1;
};

VsOut main(VsIn v)
{
    VsOut o;
    float3 p = mul(World, float4(v.Position.xyz, 1.0)) - EyePosition.xyz;
    o.Position = mul(ViewProj, float4(p, 1.0));
    o.Uv = v.Uv * TexcoordOffset.zw + TexcoordOffset.xy;
    o.Normal = mul((float3x3)World, v.Normal.xyz * 2.0 - 1.0);
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

Texture2D<float4> DiffuseMap : register(t0);
Texture2D<float4> NormalMap : register(t1);
SamplerState DiffuseSampler : register(s0);

struct PsIn
{
    float4 Position : SV_Position;
    float2 Uv : TEXCOORD0;
    float3 Normal : TEXCOORD1;
};

float4 main(PsIn i) : SV_Target
{
    float4 base = DiffuseMap.Sample(DiffuseSampler, i.Uv);
    float3 bump = NormalMap.Sample(DiffuseSampler, i.Uv).xyz * 2.0 - 1.0;
    float3 n = normalize(i.Normal + 0.001 * bump);
    float ndl = saturate(dot(n, DirLightDirection.xyz));
    float3 ambient = mul(DirectionalAmbient, float4(n, 1.0));
    // A floor of half the base colour keeps every covered pixel visible whatever the normals are.
    float3 c = base.rgb * (0.5 + 0.5 * ndl * DirLightColor.rgb + ambient);
    // Alpha below 1 on purpose: the display readback must still be opaque.
    return float4(c, base.a * 0.5);
}

#endif
