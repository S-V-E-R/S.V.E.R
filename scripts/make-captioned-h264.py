"""Embed CEA-608 roll-up captions in an Annex B H.264 stream, as OBS does (A/53 GA94 SEI).
Usage: make-captioned-h264.py in.h264 out.h264 fps (docs/COMMUNITY.md "Live captions from OBS")
Each second shows the line "CAPTION n" (n = second number). Requires access unit delimiters (x264 aud=1).
"""
import sys

def parity(b):
    return b | (0x80 if bin(b).count("1") % 2 == 0 else 0)

def pair(a, b):
    return parity(a), parity(b)

RU2, CR = (0x14, 0x25), (0x14, 0x2D)
NULL = (0x80, 0x80)

def schedule(frames, fps):
    out = []
    for f in range(frames):
        sec, k = divmod(f, fps)
        text = f"CAPTION {sec}"
        text += " " * (len(text) % 2)
        seq = [RU2, RU2, CR, CR] + [(ord(text[i]), ord(text[i + 1])) for i in range(0, len(text), 2)]
        out.append(pair(*seq[k]) if k < len(seq) else NULL)
    return out

def sei(cc):
    payload = bytes([0xB5, 0x00, 0x31]) + b"GA94" + bytes([0x03, 0x40 | 1, 0xFF, 0xFC, cc[0], cc[1], 0xFF])
    rbsp = bytes([4, len(payload)]) + payload + b"\x80"
    escaped, zeros = bytearray(), 0
    for b in rbsp:
        if zeros >= 2 and b <= 3:
            escaped.append(3)
            zeros = 0
        escaped.append(b)
        zeros = zeros + 1 if b == 0 else 0
    return b"\x00\x00\x00\x01\x06" + bytes(escaped)

def main(src, dst, fps):
    data = open(src, "rb").read()
    starts, i = [], 0
    while (i := data.find(b"\x00\x00\x01", i)) != -1:
        starts.append(i - 1 if i > 0 and data[i - 1] == 0 else i)
        i += 3
    nals = [data[s:e] for s, e in zip(starts, starts[1:] + [len(data)])]
    aus = sum(1 for n in nals if n[n.index(b"\x00\x00\x01") + 3] & 0x1F == 9)
    ccs = iter(schedule(aus, fps))
    with open(dst, "wb") as out:
        for n in nals:
            out.write(n)
            if n[n.index(b"\x00\x00\x01") + 3] & 0x1F == 9:
                out.write(sei(next(ccs)))
    print(f"access units={aus}")

main(sys.argv[1], sys.argv[2], int(sys.argv[3]))
