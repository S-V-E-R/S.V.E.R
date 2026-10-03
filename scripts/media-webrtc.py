"""Local protocol receiver, not browser automation. Test-only aiortc==1.14.0."""
import asyncio
import json
import sys
import time
import urllib.parse
import urllib.request

from aiortc import RTCBundlePolicy, RTCConfiguration, RTCPeerConnection, RTCSessionDescription
import aioice.ice


async def main(url):
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme != "http" or parsed.hostname != "127.0.0.1":
        raise ValueError("Only the loopback media fixture is allowed")
    # Test fixture only: Docker publishes media on loopback, while aioice normally
    # excludes loopback interfaces. Do not open LAN candidates for this local test.
    aioice.ice.get_host_addresses = lambda use_ipv4, use_ipv6: ["127.0.0.1"]
    peer = RTCPeerConnection(RTCConfiguration(iceServers=[], bundlePolicy=RTCBundlePolicy.MAX_BUNDLE))
    tracks = []
    counts = {"audio": 0, "video": 0}
    delays = []

    async def consume(track):
        while counts[track.kind] < 40:
            frame = await track.recv()
            counts[track.kind] += 1
            if track.kind == "video":
                gray = frame.reformat(format="gray")
                if (gray.height, gray.width) != (180, 320):
                    raise ValueError("Unexpected fixture video dimensions")
                pixels = bytes(gray.planes[0])
                stride = gray.planes[0].line_size
                stamp = sum((1 << bit) for bit in range(32) if pixels[40 * stride + bit * 10 + 5] > 128)
                now = int(time.time() * 100) & 0xFFFFFFFF
                delay = ((now - stamp) & 0xFFFFFFFF) * 10
                if delay >= 30000:
                    raise ValueError("Invalid capture clock")
                delays.append(delay)

    @peer.on("track")
    def on_track(track):
        tracks.append(asyncio.create_task(consume(track)))

    try:
        peer.addTransceiver("video", direction="recvonly")
        peer.addTransceiver("audio", direction="recvonly")
        await peer.setLocalDescription(await peer.createOffer())
        request = urllib.request.Request(url, data=peer.localDescription.sdp.encode(), headers={"Content-Type": "application/sdp"})
        def exchange():
            with urllib.request.urlopen(request, timeout=5) as response:
                return response.read(65536).decode()
        answer = await asyncio.to_thread(exchange)
        await peer.setRemoteDescription(RTCSessionDescription(sdp=answer, type="answer"))
        if len(tracks) != 2:
            raise ValueError("Missing audio or video track")
        try:
            await asyncio.wait_for(asyncio.gather(*tracks), timeout=20)
        except TimeoutError:
            raise RuntimeError(f"WebRTC timeout: connection={peer.connectionState}, ice={peer.iceConnectionState}, decoded={counts}") from None
        if counts != {"audio": 40, "video": 40}:
            raise ValueError("Incomplete decode")
        delays.sort()
        print(json.dumps({"decoded_video_frames": 40, "decoded_audio_frames": 40,
                          "capture_to_decode_p50_ms": delays[19], "capture_to_decode_p95_ms": delays[37]}))
    finally:
        for task in tracks:
            task.cancel()
        await asyncio.gather(*tracks, return_exceptions=True)
        await peer.close()


if __name__ == "__main__":
    asyncio.run(main(sys.argv[1]))
