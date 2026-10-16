using UnrealBuildTool;

public class CrowdRunnerEditorTarget : TargetRules
{
	public CrowdRunnerEditorTarget(TargetInfo Target) : base(Target)
	{
		Type = TargetType.Editor;
		DefaultBuildSettings = BuildSettingsVersion.Latest;
		IncludeOrderVersion = EngineIncludeOrderVersion.Latest;
		ExtraModuleNames.Add("CrowdRunner");
	}
}
