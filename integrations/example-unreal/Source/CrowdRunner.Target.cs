using UnrealBuildTool;

public class CrowdRunnerTarget : TargetRules
{
	public CrowdRunnerTarget(TargetInfo Target) : base(Target)
	{
		Type = TargetType.Game;
		DefaultBuildSettings = BuildSettingsVersion.Latest;
		IncludeOrderVersion = EngineIncludeOrderVersion.Latest;
		ExtraModuleNames.Add("CrowdRunner");
	}
}
