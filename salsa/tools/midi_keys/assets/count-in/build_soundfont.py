"""Build the checked-in SF2 deterministically; not a build/runtime dependency.

Run from any directory with Python 3. Inputs and licensing: NOTICE.txt.
Bank 0/program 0: MIDI notes 60..63 speak one..four, at their original pitch.
"""
from pathlib import Path
import struct

BASE = Path(__file__).resolve().parent


def chunk(tag, data):
    # SF2 strings include their even-byte NUL padding in the chunk length.
    data += bytes(len(data) % 2)
    return tag + struct.pack('<I', len(data)) + data


def name(text):
    return text.encode('ascii')[:19].ljust(20, b'\0')


def generator(op, value):
    return struct.pack('<HH', op, value & 0xffff)


def build():
    samples = bytearray()
    headers = bytearray()
    bags = bytearray()
    generators = bytearray()
    for index, word in enumerate(('one', 'two', 'three', 'four')):
        raw = (BASE / (word + '.pcm')).read_bytes()
        values = struct.unpack('<' + 'h' * (len(raw) // 2), raw)
        audible = [i for i, value in enumerate(values) if abs(value) >= 64]
        # Remove leading silence, retaining 2 ms around consonants; no pitch shift.
        begin = max(0, audible[0] - 96)
        end = min(len(values), audible[-1] + 97)
        pcm = raw[begin * 2:end * 2]
        start = len(samples) // 2
        stop = start + len(pcm) // 2
        samples.extend(pcm + bytes(46 * 2))  # SF2 interpolation guard points
        key = 60 + index
        headers.extend(struct.pack('<20sIIIIIBbHH', name(word), start, stop,
                                   start, stop, 48000, key, 0, 0, 1))
        bags.extend(struct.pack('<HH', len(generators) // 4, 0))
        generators.extend(generator(43, key | key << 8))  # keyRange first
        generators.extend(generator(34, -12000))          # immediate attack
        generators.extend(generator(38, -12000))          # short release
        generators.extend(generator(54, 0))               # no looping
        generators.extend(generator(58, key))             # original pitch
        generators.extend(generator(53, index))           # sampleID last
    bags.extend(struct.pack('<HH', len(generators) // 4, 0))
    generators.extend(generator(0, 0))
    headers.extend(struct.pack('<20sIIIIIBbHH', name('EOS'), 0, 0, 0, 0, 0, 0, 0, 0, 0))
    info = chunk(b'LIST', b'INFO' + chunk(b'ifil', struct.pack('<HH', 2, 1))
                 + chunk(b'isng', b'EMU8000\0') + chunk(b'INAM', b'Spoken count-in\0')
                 + chunk(b'IENG', b'Dvortygirl\0')
                 + chunk(b'ICOP', b'One/two/three: CC BY-SA 3.0. Four: public domain. See NOTICE.txt.\0'))
    phdr = struct.pack('<20sHHHIII', name('Spoken count-in'), 0, 0, 0, 0, 0, 0)
    phdr += struct.pack('<20sHHHIII', name('EOP'), 0, 0, 1, 0, 0, 0)
    pdta = chunk(b'LIST', b'pdta'
        + chunk(b'phdr', phdr)
        + chunk(b'pbag', struct.pack('<HHHH', 0, 0, 1, 0))
        + chunk(b'pmod', bytes(10))
        + chunk(b'pgen', generator(41, 0) + generator(0, 0))
        + chunk(b'inst', struct.pack('<20sH20sH', name('Words'), 0, name('EOI'), 4))
        + chunk(b'ibag', bags) + chunk(b'imod', bytes(10))
        + chunk(b'igen', generators) + chunk(b'shdr', headers))
    return chunk(b'RIFF', b'sfbk' + info + chunk(b'LIST', b'sdta' + chunk(b'smpl', samples)) + pdta)


if __name__ == '__main__':
    (BASE / 'count-in.sf2').write_bytes(build())
