cbuffer PerMaterial : register(b1) { float Scale; float3 Pad0; float3 Tint; float Pad1; };
Texture2D<float4> TexColor : register(t0);
TextureCube<float4> TexEnv : register(t4);
SamplerState S : register(s0);
float4 main(float4 pos : SV_Position, float2 uv : TEXCOORD0) : SV_Target { return TexColor.Sample(S, uv) * float4(Tint * Scale, 1) + TexEnv.Sample(S, float3(uv, 1)); }
