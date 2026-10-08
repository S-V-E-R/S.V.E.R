// S.V.E.R board SDK for Unity (docs/CROWDSYNC.md "Game SDK"). Add the SverBoard component to a
// GameObject, paste a game token from Creator Studio → Board → Connections, and subscribe:
//
//   var board = GetComponent<Sver.Board.SverBoard>();
//   board.Pressed += press => { if (press.Control == "jump") player.Jump(); };
//   board.Moved += move => player.Steer(move.X, move.Y);          // joystick
//   await board.SetDisabled("jump", true);                         // game → board
//
// Events arrive on the main thread (from Update). It reconnects on its own unless the token was
// revoked. Uses System.Net.WebSockets, so it runs on desktop, console and mobile players; for
// WebGL builds, use the JavaScript SDK from the page instead.
using System;
using System.Collections.Concurrent;
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
        public string gateway = "wss://sver.tv/api/integrations/ws";
        public bool connectOnStart = true;

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
        public event Action<string> Error;

        /// <summary>The latest snapshot of the published board.</summary>
        public JObject Current { get; private set; }
        public bool Connected => socket != null && socket.State == WebSocketState.Open;

        const int Revoked = 4001;
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
            if (string.IsNullOrEmpty(token)) throw new InvalidOperationException("Set a game token from Creator Studio.");
            cancel?.Cancel();
            cancel = new CancellationTokenSource();
            _ = Run(cancel.Token);
        }

        public void Disconnect()
        {
            cancel?.Cancel();
            socket?.Abort();
        }

        async Task Run(CancellationToken ct)
        {
            var delay = 1000;
            while (!ct.IsCancellationRequested)
            {
                try
                {
                    socket = new ClientWebSocket();
                    socket.Options.SetRequestHeader("Authorization", "Bearer " + token);
                    await socket.ConnectAsync(new Uri(gateway), ct);
                    delay = 1000;
                    await Receive(ct);
                }
                catch (Exception e) when (!ct.IsCancellationRequested)
                {
                    inbox.Enqueue(new JObject { ["type"] = "error", ["message"] = e.Message });
                }
                if (socket != null && (int?)socket.CloseStatus == Revoked)
                {
                    inbox.Enqueue(new JObject { ["type"] = "error", ["message"] = "The token was revoked in Creator Studio." });
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

        /// <summary>The most presses and joystick moves a second sent to the game (1–100, or null for none).</summary>
        public Task SetInputCap(int? perSecond) =>
            Send(new JObject { ["type"] = "cap", ["id"] = nextId++, ["per_second"] = perSecond.HasValue ? new JValue(perSecond.Value) : JValue.CreateNull() });

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
