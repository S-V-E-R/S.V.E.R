// Builds Crowd Runner from the command line:
//   Unity -batchmode -quit -projectPath integrations/example-unity -executeMethod Build.Windows
// The scene is made here, so the project carries no scene asset.
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;

public static class Build
{
    public static void Windows()
    {
        var scene = EditorSceneManager.NewScene(NewSceneSetup.DefaultGameObjects);
        new GameObject("Crowd Runner").AddComponent<CrowdRunner>();
        EditorSceneManager.SaveScene(scene, "Assets/Main.unity");
        var report = BuildPipeline.BuildPlayer(new[] { "Assets/Main.unity" }, "Build/CrowdRunner.exe", BuildTarget.StandaloneWindows64, BuildOptions.None);
        if (report.summary.result != UnityEditor.Build.Reporting.BuildResult.Succeeded) EditorApplication.Exit(1);
    }
}
