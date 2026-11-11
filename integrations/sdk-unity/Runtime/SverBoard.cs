// S.V.E.R board SDK for Unity (docs/CROWDSYNC.md "Game SDK"). Add the SverBoard component to a
// GameObject, paste a game token from Creator Studio → Board → Connections, and subscribe:
//
//   var board = GetComponent<Sver.Board.SverBoard>();
//   board.Pressed += press => { if (press.Control == "jump") player.Jump(); };
//   board.Moved += move => player.Steer(move.X, move.Y);          // joystick
//   await board.SetDisabled("jump", true);                         // game → board
//
// "Connect to S.V.E.R" instead of a pasted token: register the game once in Settings → Developer
// (no client secret), leave the token empty and set clientId. Connect() then raises SignInCode
// with a code for the streamer to enter at sver.tv/go, keeps the sign-in (PlayerPrefs, or your
// LoadSignIn/SaveSignIn) and renews it on its own; a new code comes only when the sign-in is gone.
//
//   board.SignInCode += (code, url) => label.text = $"Enter {code} at {url}";
//
// Events arrive on the main thread (from Update). It reconnects on its own unless the token was
// revoked. Uses System.Net.WebSockets, so it runs on desktop, console and mobile players; for
// WebGL builds, use the JavaScript SDK from the page instead.
using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Net.Http;
using System.Net.WebSockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using UnityEngine;

namespace Sver.Board
{
    /// <summary>A viewer pressed a control (a button, a goal or a text input).</summary>
    public sealed class Press
    {
        /// <summary>The press, for Capture and Release.</summary>
        public string Id;
        /// <summary>A "Game confirms" control: call Capture(Id) once it happened, or Release(Id).</summary>
        public bool Confirm;
        public string Control;
        public string Label;
        public string Username;
        /// <summary>What the viewer typed, for text inputs; otherwise null.</summary>
        public string Text;
        public JObject Raw;
    }

    /// <summary>A viewer moved a joystick control (X and Y from -1 to 1).</summary>
    public sealed class Move
    {
        public string Control;
        public string Username;
        public float X;
        public float Y;
    }

    public sealed class SverBoard : MonoBehaviour
    {
        [Tooltip("A game token from Creator Studio → Board → Connections. Keep it out of source control.")]
        public string token;
        [Tooltip("\"Connect to S.V.E.R\": your app's client ID from Settings → Developer, used when the token is empty. The streamer enters a code at sver.tv/go.")]
        public string clientId;
        public string gateway = "wss://sver.tv/api/integrations/ws";
        public string api = "https://sver.tv";
        public bool connectOnStart = true;

        /// <summary>Show the streamer this code and address (code, url): "Enter BCD-FGH at https://sver.tv/go".</summary>
        public event Action<string, string> SignInCode;
        /// <summary>Where the sign-in (a refresh token) is kept. PlayerPrefs unless both are set; SaveSignIn(null) forgets it.</summary>
        public Func<string> LoadSignIn;
        public Action<string> SaveSignIn;

        /// <summary>Connected: the channel and its published board ("board", "version", "state", "goals").</summary>
        public event Action<JObject> Hello;
        public event Action<Press> Pressed;
        public event Action<Move> Moved;
        /// <summary>A Skill, emote combo or Surge celebration played (no control).</summary>
        public event Action<JObject> Effect;
        /// <summary>A new board version was published, or the board was paused.</summary>
        public event Action<JObject> BoardChanged;
        /// <summary>Labels, availability or goal progress changed.</summary>
        public event Action<JObject> StateChanged;
        /// <summary>A "Game confirms" press was captured or released ("id", "control", "outcome").</summary>
        public event Action<JObject> Result;
        public event Action<string> Error;

        /// <summary>The latest snapshot of the published board.</summary>
        public JObject Current { get; private set; }
        public bool Connected => socket != null && socket.State == WebSocketState.Open;

        const int Revoked = 4001;
        static readonly HttpClient http = new HttpClient();
        /// <summary>Trying again won't help: the streamer chose Cancel, or S.V.E.R doesn't know the app.</summary>
        sealed class Refused : Exception { public Refused(string message) : base(message) { } }
        bool SignsIn => string.IsNullOrEmpty(token);
        string access;
        DateTime accessUntil;
        readonly ConcurrentQueue<JObject> inbox = new ConcurrentQueue<JObject>();
        readonly SemaphoreSlim sendLock = new SemaphoreSlim(1, 1);
        ClientWebSocket socket;
        CancellationTokenSource cancel;
        int nextId = 1;

        void Start()
        {
            if (connectOnStart) Connect();
        }

        public void Connect()
        {
            if (SignsIn && string.IsNullOrEmpty(clientId)) throw new InvalidOperationException("Set a game token from Creator Studio, or your app's client ID.");
            cancel?.Cancel();
            cancel = new CancellationTokenSource();
            _ = Run(cancel.Token);
        }

        public void Disconnect()
        {
            cancel?.Cancel();
            socket?.Abort();
        }

        /// <summary>Disconnects and forgets the sign-in, here and on S.V.E.R; the next Connect() shows a new code.</summary>
        public void SignOut()
        {
            Disconnect();
            var refresh = Load();
            Save(null);
            access = null;
            if (!string.IsNullOrEmpty(refresh)) _ = OAuth("revoke", CancellationToken.None, ("token", refresh));
        }

        // Unity resumes these awaits on the main thread, where PlayerPrefs has to be used.
        string Load() => LoadSignIn != null && SaveSignIn != null ? LoadSignIn() : PlayerPrefs.GetString("sver-board:" + clientId, "");

        void Save(string refresh)
        {
            if (LoadSignIn != null && SaveSignIn != null) { SaveSignIn(refresh); return; }
            if (string.IsNullOrEmpty(refresh)) PlayerPrefs.DeleteKey("sver-board:" + clientId);
            else PlayerPrefs.SetString("sver-board:" + clientId, refresh);
            PlayerPrefs.Save();
        }

        // The OAuth endpoints take a form and no secret: the client ID is public. A 400 carries {"error"}.
        async Task<JObject> OAuth(string path, CancellationToken ct, params (string, string)[] fields)
        {
            var form = new Dictionary<string, string> { ["client_id"] = clientId };
            foreach (var (key, value) in fields) form[key] = value;
            using (var response = await http.PostAsync(api.TrimEnd('/') + "/api/oauth/" + path, new FormUrlEncodedContent(form), ct))
            {
                if (!response.IsSuccessStatusCode && (int)response.StatusCode != 400)
                    throw new InvalidOperationException("S.V.E.R sign-in isn't available right now (" + (int)response.StatusCode + ").");
                return JObject.Parse(await response.Content.ReadAsStringAsync());
            }
        }

        /// <summary>A working access token: the one in hand, a renewed one, or a new sign-in at sver.tv/go.</summary>
        async Task<string> AccessToken(CancellationToken ct)
        {
            if (access != null && DateTime.UtcNow < accessUntil) return access;
            var refresh = Load();
            var tokens = string.IsNullOrEmpty(refresh) ? null : await OAuth("token", ct, ("grant_type", "refresh_token"), ("refresh_token", refresh));
            if (tokens == null || tokens["error"] != null)
            {
                // No sign-in, or it's gone (removed in Settings → Connected apps, unused for 60 days): a new code.
                Save(null);
                tokens = await Device(ct);
            }
            access = (string)tokens["access_token"];
            accessUntil = DateTime.UtcNow.AddSeconds((double)tokens["expires_in"] - 60);
            Save((string)tokens["refresh_token"]);
            return access;
        }

        /// <summary>Device sign-in (RFC 8628): shows a code, then asks no faster than S.V.E.R's interval until it is approved.</summary>
        async Task<JObject> Device(CancellationToken ct)
        {
            while (true)
            {
                var start = await OAuth("device", ct, ("scope", "board:control"));
                if (start["error"] != null) throw new Refused("S.V.E.R doesn't know this app (" + (string)start["error"] + "). Check the client ID.");
                inbox.Enqueue(new JObject { ["type"] = "code", ["code"] = start["user_code"], ["url"] = start["verification_uri"] });
                var wait = (double?)start["interval"] ?? 5;
                while (true)
                {
                    // A little over the interval, so a request that travels faster than the last isn't "too soon".
                    await Task.Delay(TimeSpan.FromSeconds(wait + 0.25), ct);
                    var tokens = await OAuth("token", ct, ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"), ("device_code", (string)start["device_code"]));
                    var error = (string)tokens["error"];
                    if (error == null) return tokens;
                    if (error == "slow_down") wait += 5;
                    else if (error == "expired_token") break; // nobody entered it in time: show a new code
                    else if (error != "authorization_pending")
                        throw new Refused(error == "access_denied" ? "The sign-in was cancelled at sver.tv/go." : "S.V.E.R refused the sign-in (" + error + ").");
                }
            }
        }

        async Task Run(CancellationToken ct)
        {
            var delay = 1000;
            while (!ct.IsCancellationRequested)
            {
                try
                {
                    var bearer = SignsIn ? await AccessToken(ct) : token;
                    socket = new ClientWebSocket();
                    socket.Options.SetRequestHeader("Authorization", "Bearer " + bearer);
                    if (SignsIn) socket.Options.SetRequestHeader("SVER-Client-Id", clientId);
                    await socket.ConnectAsync(new Uri(gateway), ct);
                    delay = 1000;
                    await Receive(ct);
                }
                catch (Refused e)
                {
                    inbox.Enqueue(new JObject { ["type"] = "error", ["message"] = e.Message });
                    return;
                }
                catch (Exception e) when (!ct.IsCancellationRequested)
                {
                    // Refused before hello, perhaps: a signed-in game renews its access before the next try.
                    if (socket == null || socket.State != WebSocketState.Open) access = null;
                    inbox.Enqueue(new JObject { ["type"] = "error", ["message"] = e.Message });
                }
                if (socket != null && (int?)socket.CloseStatus == Revoked)
                {
                    // A signed-in game forgets the sign-in; its next Connect() shows a new code.
                    if (SignsIn) { Save(null); access = null; }
                    inbox.Enqueue(new JObject { ["type"] = "error", ["message"] = SignsIn ? "This game was removed in Settings → Connected apps on S.V.E.R." : "The token was revoked in Creator Studio." });
                    return;
                }
                try { await Task.Delay(delay, ct); } catch (TaskCanceledException) { return; }
                delay = Math.Min(delay * 2, 30000);
            }
        }

        async Task Receive(CancellationToken ct)
        {
            var buffer = new byte[16 * 1024];
            var text = new StringBuilder();
            while (socket.State == WebSocketState.Open)
            {
                var result = await socket.ReceiveAsync(new ArraySegment<byte>(buffer), ct);
                if (result.MessageType == WebSocketMessageType.Close) return;
                text.Append(Encoding.UTF8.GetString(buffer, 0, result.Count));
                if (!result.EndOfMessage) continue;
                try { inbox.Enqueue(JObject.Parse(text.ToString())); } catch (JsonException) { }
                text.Clear();
            }
        }

        // Events are raised on Unity's main thread.
        void Update()
        {
            while (inbox.TryDequeue(out var message)) Dispatch(message);
        }

        void Dispatch(JObject message)
        {
            switch ((string)message["type"])
            {
                case "hello":
                    Current = message;
                    Hello?.Invoke(message);
                    break;
                case "board_effect":
                    if (message["control"] == null) { Effect?.Invoke(message); break; }
                    Pressed?.Invoke(new Press
                    {
                        Id = (string)message["id"],
                        Confirm = (bool?)message["confirm"] ?? false,
                        Control = (string)message["control"],
                        Label = (string)message["label"],
                        Username = (string)message["user"]?["username"],
                        Text = message["text"]?.Type == JTokenType.String ? (string)message["text"] : null,
                        Raw = message,
                    });
                    break;
                case "board_input":
                    Moved?.Invoke(new Move
                    {
                        Control = (string)message["control"],
                        Username = (string)message["user"]?["username"],
                        X = (float?)message["x"] ?? 0,
                        Y = (float?)message["y"] ?? 0,
                    });
                    break;
                case "board":
                    Current = message;
                    BoardChanged?.Invoke(message);
                    break;
                case "board_state":
                    if (Current != null)
                    {
                        Current["state"] = message["state"];
                        Current["goals"] = message["goals"];
                    }
                    StateChanged?.Invoke(message);
                    break;
                case "board_result":
                    Result?.Invoke(message);
                    break;
                case "code":
                    SignInCode?.Invoke((string)message["code"], (string)message["url"]);
                    break;
                case "error":
                    Error?.Invoke((string)message["message"]);
                    break;
            }
        }

        /// <summary>
        /// Changes the published board: per control id, "label" (text, or null to restore),
        /// "disabled" and, for goals, "progress". At most 10 messages a second; a rejected change
        /// arrives as an Error event.
        /// </summary>
        public Task SetState(JObject controls) =>
            Send(new JObject { ["type"] = "state", ["id"] = nextId++, ["controls"] = controls });

        public Task SetDisabled(string control, bool disabled) =>
            SetState(new JObject { [control] = new JObject { ["disabled"] = disabled } });

        public Task SetLabel(string control, string label) =>
            SetState(new JObject { [control] = new JObject { ["label"] = label == null ? JValue.CreateNull() : new JValue(label) } });

        public Task SetProgress(string control, int progress) =>
            SetState(new JObject { [control] = new JObject { ["progress"] = progress } });

        /// <summary>The game is listening. Viewers see "Starting…" from each connect until this; call it on every Hello.</summary>
        public Task Ready() => Send(new JObject { ["type"] = "ready", ["id"] = nextId++ });

        /// <summary>Shows each group one screen: by "faction" ({faction: screen}), "random" ([screens]) or "users" ({username: screen}); by null clears.</summary>
        public Task SetGroups(string by, JToken screens) =>
            Send(new JObject { ["type"] = "groups", ["id"] = nextId++, ["by"] = by, ["screens"] = screens });

        /// <summary>A "Game confirms" press happened: the viewer is charged.</summary>
        public Task Capture(string press) => Send(new JObject { ["type"] = "capture", ["id"] = nextId++, ["press"] = press });

        /// <summary>It didn't happen: the viewer is refunded (automatic after 60 seconds).</summary>
        public Task Release(string press) => Send(new JObject { ["type"] = "release", ["id"] = nextId++, ["press"] = press });

        /// <summary>The most presses and joystick moves a second sent to the game (1–100, or null for none).</summary>
        public Task SetInputCap(int? perSecond) =>
            Send(new JObject { ["type"] = "cap", ["id"] = nextId++, ["per_second"] = perSecond.HasValue ? new JValue(perSecond.Value) : JValue.CreateNull() });

        /// <summary>
        /// Makes this game's own board the live board while the game is connected, if the streamer switched on
        /// "May set my board" for this connection (otherwise Error fires with the reason). board is {"screens":[{"name","controls":[…]}]}.
        /// Call it again on every Hello; at most once every 10 seconds.
        /// </summary>
        public Task Define(string name, JObject board) =>
            Send(new JObject { ["type"] = "define", ["id"] = nextId++, ["name"] = name, ["board"] = board });

        async Task Send(JObject message)
        {
            if (!Connected) throw new InvalidOperationException("Not connected to S.V.E.R.");
            var bytes = Encoding.UTF8.GetBytes(message.ToString(Formatting.None));
            await sendLock.WaitAsync();
            try { await socket.SendAsync(new ArraySegment<byte>(bytes), WebSocketMessageType.Text, true, cancel.Token); }
            finally { sendLock.Release(); }
        }

        void OnDestroy() => Disconnect();
    }
}
