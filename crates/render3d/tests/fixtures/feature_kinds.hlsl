// P1-C Task 2: a FeatureData-shaped cbuffer (namespaced like CS's SharedData.hlsli, struct members of each scalar
// type) for the reflected-kind tests in tests/compile.rs.
namespace SharedData
{
	struct HairSpecularSettings
	{
		uint Enabled;
		float HairGlossiness;
		int Signed;
		bool Flag;
	};

	cbuffer FeatureData : register(b6)
	{
		HairSpecularSettings hairSpecularSettings;
		float4 skinParams;
	};
}

float4 main(float4 pos : SV_Position) : SV_Target
{
	float g = SharedData::hairSpecularSettings.HairGlossiness + SharedData::hairSpecularSettings.Signed +
	          (SharedData::hairSpecularSettings.Flag ? 1.0 : 0.0);
	return SharedData::hairSpecularSettings.Enabled ? SharedData::skinParams * g : 0.0;
}
