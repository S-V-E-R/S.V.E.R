// S.V.E.R board SDK for Unreal Engine 5 (docs/CROWDSYNC.md "Game SDK").
using UnrealBuildTool;

public class SverBoard : ModuleRules
{
	public SverBoard(ReadOnlyTargetRules Target) : base(Target)
	{
		PCHUsage = ModuleRules.PCHUsageMode.UseExplicitOrSharedPCHs;
		PublicDependencyModuleNames.AddRange(new string[] { "Core", "CoreUObject", "Engine" });
		PrivateDependencyModuleNames.AddRange(new string[] { "WebSockets", "Json" });
	}
}
