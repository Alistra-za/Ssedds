// Repo-owned stand-in for the Community Shaders DeferredCompositeCS.hlsl (written for sdc's render3d tests; not
// derived from CS). It declares the names and registers render3d binds by reflection (b5 SharedData BufferDim, t0
// SpecularTexture, t2 NormalRoughnessTexture, t4 DepthTexture, u0 MainRW, u1 NormalTAAMaskSpecularMaskRW), so the
// deferred composite runs on WARP without the user's CS install (v2 D4). GPL-3.0-or-later, like the rest of sdc.
//
// Without STANDIN_COMPOSITE_ADD it is a passthrough (MainRW keeps its colour, alpha becomes 1), so the D3 deferred
// frame tests stay valid. With it, MainRW.rgb += SpecularTexture x NormalRoughnessTexture.x + DepthTexture (an
// unbound DepthTexture reads 0). STANDIN_COMPOSITE_BROKEN is a compile error (the fallback tests).

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

RWTexture2D<float4> MainRW : register(u0);
RWTexture2D<float4> NormalTAAMaskSpecularMaskRW : register(u1);

[numthreads(8, 8, 1)] void main(uint3 dispatchID : SV_DispatchThreadID) {
	// As CS: an unwritten BufferDim makes every thread exit.
	if (any(dispatchID.xy >= uint2(BufferDim.xy)))
		return;
	float3 c = MainRW[dispatchID.xy].xyz;
#if defined(STANDIN_COMPOSITE_ADD)
	c += SpecularTexture[dispatchID.xy] * NormalRoughnessTexture[dispatchID.xy].x + DepthTexture[dispatchID.xy];
#endif
	MainRW[dispatchID.xy] = float4(c, 1.0);
	NormalTAAMaskSpecularMaskRW[dispatchID.xy] = 1.0;
}
