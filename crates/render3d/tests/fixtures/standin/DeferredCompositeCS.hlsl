// Repo-owned stand-in for the Community Shaders DeferredCompositeCS.hlsl (written for sdc's render3d tests; not
// derived from CS). It declares the names and registers render3d binds by reflection (b5 SharedData BufferDim, t0
// SpecularTexture, t2 NormalRoughnessTexture, t4 DepthTexture, u0 MainRW, u1 NormalTAAMaskSpecularMaskRW; under
// DYNAMIC_CUBEMAPS t5 ReflectanceTexture, t6 EnvTexture, t7 EnvReflectionsTexture, s0 LinearSampler), so the deferred
// composite runs on WARP without the user's CS install (v2 D4, D5). GPL-3.0-or-later, like the rest of sdc.
//
// Without defines it is a passthrough (MainRW keeps its colour, alpha becomes 1), so the D3 deferred frame tests stay
// valid. DepthTexture is always read, but outside STANDIN_COMPOSITE_DEPTH it reaches only u1 (as CS reads depth
// outside DYNAMIC_CUBEMAPS only for MotionVectorsRW), so t4 is reflected and bound and MainRW does not see it.
// - STANDIN_COMPOSITE_ADD: MainRW.rgb += SpecularTexture x NormalRoughnessTexture.x.
// - STANDIN_COMPOSITE_DEPTH: MainRW.rgb += DepthTexture.
// - DYNAMIC_CUBEMAPS: MainRW.rgb += ReflectanceTexture x EnvReflectionsTexture's level-15 +Z sample (EnvTexture is
//   declared and not read, so it is stripped as in CS's own permutation).
// STANDIN_COMPOSITE_BROKEN is a compile error (the fallback tests).

#if !defined(CSHADER)
#	error CSHADER is not defined
#endif
#if defined(STANDIN_COMPOSITE_BROKEN)
#	error STANDIN_COMPOSITE_BROKEN
#endif

cbuffer SharedData : register(b5)
{
	float4 BufferDim;
};

Texture2D<float3> SpecularTexture : register(t0);
Texture2D<unorm float3> NormalRoughnessTexture : register(t2);
Texture2D<unorm float> DepthTexture : register(t4);

#if defined(DYNAMIC_CUBEMAPS)
Texture2D<float3> ReflectanceTexture : register(t5);
TextureCube<float3> EnvTexture : register(t6);
TextureCube<float3> EnvReflectionsTexture : register(t7);
SamplerState LinearSampler : register(s0);
#endif

RWTexture2D<float4> MainRW : register(u0);
RWTexture2D<float4> NormalTAAMaskSpecularMaskRW : register(u1);

[numthreads(8, 8, 1)] void main(uint3 dispatchID : SV_DispatchThreadID) {
	// As CS: an unwritten BufferDim makes every thread exit.
	if (any(dispatchID.xy >= uint2(BufferDim.xy)))
		return;
	float3 c = MainRW[dispatchID.xy].xyz;
	float depth = DepthTexture[dispatchID.xy];
#if defined(STANDIN_COMPOSITE_ADD)
	c += SpecularTexture[dispatchID.xy] * NormalRoughnessTexture[dispatchID.xy].x;
#endif
#if defined(STANDIN_COMPOSITE_DEPTH)
	c += depth;
#endif
#if defined(DYNAMIC_CUBEMAPS)
	c += ReflectanceTexture[dispatchID.xy] * EnvReflectionsTexture.SampleLevel(LinearSampler, float3(0.0, 0.0, 1.0), 15);
#endif
	MainRW[dispatchID.xy] = float4(c, 1.0);
	NormalTAAMaskSpecularMaskRW[dispatchID.xy] = depth == 1.0 ? 1.0 : 0.5;
}
