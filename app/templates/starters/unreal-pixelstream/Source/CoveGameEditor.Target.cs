using UnrealBuildTool;

// The cook runs the editor on this project, so a C++ project needs its
// editor target too.
public class CoveGameEditorTarget : TargetRules
{
	public CoveGameEditorTarget(TargetInfo Target) : base(Target)
	{
		Type = TargetType.Editor;
		DefaultBuildSettings = BuildSettingsVersion.V5;
		IncludeOrderVersion = EngineIncludeOrderVersion.Latest;
		ExtraModuleNames.Add("CoveGame");
	}
}
