using UnrealBuildTool;

public class CrowdRunner : ModuleRules
{
	public CrowdRunner(ReadOnlyTargetRules Target) : base(Target)
	{
		PCHUsage = PCHUsageMode.UseExplicitOrSharedPCHs;
		PublicDependencyModuleNames.AddRange(new string[] { "Core", "CoreUObject", "Engine", "SverBoard" });
		PrivateDependencyModuleNames.AddRange(new string[] { "HTTP" });
	}
}
