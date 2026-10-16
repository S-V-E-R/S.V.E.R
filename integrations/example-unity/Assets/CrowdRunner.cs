// Crowd Runner for Unity: the browser example game (integrations/example-game) on the Unity SDK.
// Viewers make the runner jump, change its speed with a joystick and shout; the game disables
// Jump while the runner is airborne and fills the coins goal as it runs.
//
// Board controls: a button "jump", a goal "coins", a text input "shout", a joystick "move", and
// optionally a "Game confirms" button "boost" (captured when the boost happens, else released).
//
// Token: the -sverToken argument or SVER_TOKEN. Gateway: -sverGateway or SVER_GATEWAY (default
// wss://sver.tv/api/integrations/ws). Without a token it plays locally: Space jumps.
using System;
using Sver.Board;
using UnityEngine;

public sealed class CrowdRunner : MonoBehaviour
{
    const float Gravity = -30f, JumpSpeed = 12f;
    SverBoard board;
    Transform runner;
    float height, rise, speed = 4f, distance, boostUntil;
    int collected;
    bool airborne;
    string shout = "";
    float shoutUntil;

    void Awake()
    {
        Camera.main.transform.SetPositionAndRotation(new Vector3(0, 3, -10), Quaternion.Euler(10, 0, 0));
        var ground = GameObject.CreatePrimitive(PrimitiveType.Cube);
        ground.transform.localScale = new Vector3(40, 1, 4);
        ground.transform.position = new Vector3(0, -1, 0);
        runner = GameObject.CreatePrimitive(PrimitiveType.Cube).transform;

        var token = Arg("-sverToken") ?? Environment.GetEnvironmentVariable("SVER_TOKEN");
        if (string.IsNullOrEmpty(token)) { Log("No token: playing locally. Space jumps."); return; }
        board = gameObject.AddComponent<SverBoard>();
        board.connectOnStart = false;
        board.token = token;
        board.gateway = Arg("-sverGateway") ?? Environment.GetEnvironmentVariable("SVER_GATEWAY") ?? board.gateway;
        board.Hello += hello =>
        {
            Log($"Connected to {hello["channel"]}'s board (version {hello["version"]}).");
            _ = board.Ready();
            _ = board.SetProgress("coins", collected);
        };
        board.Pressed += press =>
        {
            Log($"Press {press.Control} by {press.Username}{(press.Text != null ? $": {press.Text}" : "")}");
            switch (press.Control)
            {
                case "jump": Jump(); break;
                case "shout": if (press.Text != null) { shout = $"{press.Username}: {press.Text}"; shoutUntil = Time.time + 4; } break;
                case "boost":
                    // A "Game confirms" control: charge the viewer only if the boost happened.
                    if (press.Confirm && Time.time < boostUntil) { _ = board.Release(press.Id); break; }
                    boostUntil = Time.time + 3;
                    if (press.Confirm) _ = board.Capture(press.Id);
                    break;
            }
        };
        board.Moved += move =>
        {
            if (move.Control != "move") return;
            speed = Mathf.Clamp(speed + move.X, 2, 9);
            Log($"{move.Username} set the speed to {speed:0.0}.");
        };
        board.Result += result => Log($"Press {result["id"]} {result["outcome"]}");
        board.Error += message => Log($"Board error: {message}");
        board.Connect();
    }

    void Update()
    {
        if (board == null && Input.GetKeyDown(KeyCode.Space)) Jump();
        var boost = Time.time < boostUntil ? 2f : 1f;
        distance += speed * boost * Time.deltaTime;
        if (airborne)
        {
            rise += Gravity * Time.deltaTime;
            height = Mathf.Max(0, height + rise * Time.deltaTime);
            if (height == 0) { airborne = false; SyncJump(); Log("Landed."); }
        }
        runner.position = new Vector3(0, height, 0);
        var coins = (int)(distance / 10);
        if (coins != collected)
        {
            collected = coins;
            if (board != null && board.Connected) _ = board.SetProgress("coins", Mathf.Min(collected, 10));
        }
        if (Time.time > shoutUntil) shout = "";
    }

    void Jump()
    {
        if (airborne) return;
        airborne = true;
        rise = JumpSpeed;
        SyncJump();
        Log("Jumped.");
    }

    // Viewers can't press Jump while the runner is in the air.
    void SyncJump()
    {
        if (board == null || !board.Connected) return;
        _ = board.SetState(new Newtonsoft.Json.Linq.JObject
        {
            ["jump"] = new Newtonsoft.Json.Linq.JObject
            {
                ["disabled"] = airborne,
                ["label"] = airborne ? "Jump (in the air)" : null,
            },
        });
    }

    void OnGUI()
    {
        GUI.Label(new Rect(10, 10, 600, 24), $"Coins {collected}  Speed {speed:0.0}{(Time.time < boostUntil ? "  BOOST" : "")}");
        if (shout.Length > 0) GUI.Label(new Rect(10, 34, 600, 24), shout);
    }

    static string Arg(string name)
    {
        var args = Environment.GetCommandLineArgs();
        var i = Array.IndexOf(args, name);
        return i >= 0 && i + 1 < args.Length ? args[i + 1] : null;
    }

    static void Log(string message) => Debug.Log($"[CrowdRunner] {message}");
}
