"""Direct-call records for the rally-school judging core (E19 filter bank, 0x497b10-0x499360).

The bank is built in the emulator's stub heap (a 0x60-byte bank, sub/check/pair arrays, the gameflow/app/message/
signal/animset/gate objects it reads), so every record is self-contained: the test rebuilds the inputs from the
argument words and the windows. Emits (0x498c70/0x498d30/0x477920), the class gate (0x49a0f0), the driveline sample
(0x50b180) and the repair-part reads (0x4f2b40) stay listed as externals, so each record carries the payloads and
gate results the port takes as explicit inputs.

    harness/direct_calls.py SNAPSHOT OUT.txt --set gameplay_school [--addr 0xADDR]... [--count N] [--seed S]

Needs a snapshot with a drive at 0x1659184 (e.g. $RBR_SNAPS/snapH.snap-1295.bin); the gameflow, app, message owner,
signal manager, animset and class-gate host are all stubbed in the heap.
"""

import struct  # noqa: E402 (the recipes pack words)

SET_NAME = "gameplay_school"

GAMEFLOW_PTR = 0x0165FA90  # the gameflow object (*M00565470 is school, +0xca8 the substate)
APP_PTR = 0x7EAC48         # the application pointer (+0x728 is the app state)
MSG_PTR = 0x0165FC68       # the message owner (+0x140 is the HUD time / Publish target)
SIGNALS_PTR = 0x00893600   # the signal manager (incident +0xa998, sound +0x8a30, endrace +0x7970)
ANIM_PTR = 0x7EABA8        # the animset (BlockAddB adds +0x3c)
DRIVE_PTR = 0x01659184     # the drive line (Sample reads it)
GATE_PTR = 0x00893634      # the class-gate host

H_BANK = 0x10000      # the 0x60-byte bank
H_SUBS = 0x10100      # 6 x 0x2c sub slots
H_CHKS = 0x10200      # 4 x 0x20 check slots
H_PAIRS = 0x10300     # 5 x 8 pair slots
H_GAMEFLOW = 0x10400  # 0xD00 (covers +0xcb4)
H_APP = 0x11200       # 0x800 (covers +0x728)
H_MSG = 0x11A00       # 0x200 (covers +0x140)
H_MGR = 0x11C00       # 0xB000 (the manager with three neutered buses)
H_COUNT = 0x11C00     # the buses' shared slot-count word (0: slot 0, no listeners)
H_ANIM = 0x1CD00      # 0x40 (covers +0x3c)
H_HOST = 0x1CE00      # 0x9D00 (M00565f90's 0x7b host)
H_VT = 0x26B00        # the part vtable ([Get20])
H_HANDLER = 0x26C00   # 0x408 (m_handler + 256 part slots)
H_PARTS = 0x27008     # 256 x 0x24 part objects (8 bytes past the handler's last slot)
H_READER = 0x29600    # the E19Reader (cursor + stream)
H_STREAM = 0x29700    # 101 stream words
H_VEC = 0x29A00       # 0x40 tick/incident sample words
H_CKPT = 0x29B00      # 0x20 checkpoint record
H_VF00 = 0x29D00       # the part Vf00 stub: damage at +0x20, pops its arg
H_GOP = 0x80000       # 0x18C000 class-gate object

MAX_SUBS, MAX_CHKS, MAX_PAIRS = 6, 4, 5

FLOATS = (0.0, -0.0, 1.0, -1.0, 0.04, 0.039, 0.041, 0.99, 1.0, 1.01, 2.0, 4.0, 5.0, 9.0, 9.5, 10.0, 10.5, 11.0,
          12.0, 13.6, 17.0, 19.0, 20.0, 25.0, 44.0, 50.0, 79.9, 100.0, 102.0, 113.314, 1e-6, 1e10, -1e10,
          float("inf"), float("-inf"), float("nan"))
INTS = (0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 24, 37, 65, 73, -1, -2, 0x7FFFFFFF, -0x80000000, 0xFFFFFFFF)


def word(mu, at):
    return struct.unpack("<I", bytes(mu.mem_read(at, 4)))[0]


def fbits(value):
    return struct.unpack("<I", struct.pack("<f", value))[0]


def _float(rng):
    r = rng.random()
    if r < 0.6:
        return rng.choice(FLOATS)
    if r < 0.8:
        return rng.uniform(-200.0, 200.0)
    return struct.unpack("<f", struct.pack("<I", rng.getrandbits(32)))[0]


def _sub_words(rng):
    """One fuzzed 11-word sub: mode tags, int/float-bit class words, windows, flags, limits."""
    tag = rng.choice((0, 1, 2, 3, 4, 5, 6, 7, 8, 9, -2, -1, 37, 0x7FFFFFFF, rng.getrandbits(32)))
    value = rng.choice((0, 0, 1, 2, 3, 4, -1, rng.getrandbits(32)))
    if rng.random() < 0.5:
        cls = rng.choice((0, 1, 2, 3, 4, rng.getrandbits(32)))
    else:
        cls = fbits(rng.choice((1.0, 79.9, 36.4, rng.uniform(-100.0, 100.0))))
    center, hi, lo = _float(rng), _float(rng), _float(rng)
    flags = rng.choice((0, 0, 1, 2, 3, 4, 5, 8, 12, 0xFFFFFFFF, rng.getrandbits(32)))
    if rng.random() < 0.5:
        near = rng.choice((0, 1, 0xFFFFFFFF, rng.getrandbits(32)))
    else:
        near = fbits(_float(rng))
    lims = [rng.choice((0, -2, -1, 10, 24, 37, rng.getrandbits(32))) for _ in range(3)]
    # Word order: tag, value, class, flags, center, hi, lo, near, lim0, lim1, lim2.
    return [tag & 0xFFFFFFFF, value & 0xFFFFFFFF, cls & 0xFFFFFFFF, flags & 0xFFFFFFFF, fbits(center),
            fbits(hi), fbits(lo), near, lims[0] & 0xFFFFFFFF, lims[1] & 0xFFFFFFFF,
            lims[2] & 0xFFFFFFFF]


def _bank_writes(heap, rng, nsub=None, nchk=None, npair=None):
    """The bank at H_BANK with fuzzed arrays: [(address, bytes)] and (nsub, nchk, npair)."""
    if nsub is None:
        nsub = rng.randrange(0, MAX_SUBS + 1)
    if nchk is None:
        nchk = rng.randrange(0, MAX_CHKS + 1)
    if npair is None:
        npair = rng.randrange(0, MAX_PAIRS + 1)
    bank = bytearray(0x60)
    struct.pack_into("<I", bank, 0x00, rng.choice((0, rng.choice(INTS))) & 0xFFFFFFFF)
    struct.pack_into("<f", bank, 0x04, _float(rng))
    struct.pack_into("<f", bank, 0x08, rng.choice((0.0, 9.0, 10.0, 11.0, _float(rng))))
    struct.pack_into("<f", bank, 0x0C, rng.choice((0.0, 9.0, 10.0, 11.0, _float(rng))))
    struct.pack_into("<f", bank, 0x10, rng.choice((0.0, 9.0, 10.0, 11.0, _float(rng))))
    struct.pack_into("<I", bank, 0x14, nsub)
    struct.pack_into("<I", bank, 0x18, heap + H_SUBS if nsub else 0)
    struct.pack_into("<I", bank, 0x2C, nchk)
    struct.pack_into("<I", bank, 0x30, heap + H_CHKS if nchk else 0)
    struct.pack_into("<I", bank, 0x38, npair)
    struct.pack_into("<I", bank, 0x3C, heap + H_PAIRS if npair else 0)
    struct.pack_into("<I", bank, 0x40, rng.getrandbits(32))
    struct.pack_into("<I", bank, 0x44, rng.choice((0, 0, 1, rng.choice(INTS))) & 0xFFFFFFFF)
    struct.pack_into("<I", bank, 0x48, rng.choice((0, 0, 1, rng.choice(INTS))) & 0xFFFFFFFF)
    struct.pack_into("<I", bank, 0x4C, rng.choice(INTS) & 0xFFFFFFFF)
    struct.pack_into("<f", bank, 0x50, rng.choice((0.0, 10.0, -1.0, _float(rng))))
    struct.pack_into("<i", bank, 0x54, rng.choice((0, 0, 1)))
    struct.pack_into("<i", bank, 0x58, rng.choice((0, 1, 1)))
    subs = bytearray(MAX_SUBS * 0x2C)
    for i in range(MAX_SUBS):
        words = _sub_words(rng) if i < nsub else [0] * 11
        struct.pack_into("<11I", subs, i * 0x2C, *words)
    chks = bytearray(MAX_CHKS * 0x20)
    for i in range(MAX_CHKS):
        if i < nchk:
            struct.pack_into("<f", chks, i * 0x20 + 0x00, _float(rng))
            struct.pack_into("<f", chks, i * 0x20 + 0x04, rng.choice((44.0, 100.0, 113.314, _float(rng))))
            struct.pack_into("<f", chks, i * 0x20 + 0x08, rng.choice((2.6, 5.0, _float(rng))))
            struct.pack_into("<I", chks, i * 0x20 + 0x0C, rng.choice((0, 1, rng.choice(INTS))) & 0xFFFFFFFF)
            struct.pack_into("<I", chks, i * 0x20 + 0x10, rng.choice(INTS) & 0xFFFFFFFF)
            struct.pack_into("<f", chks, i * 0x20 + 0x14, _float(rng))
            struct.pack_into("<f", chks, i * 0x20 + 0x18, _float(rng))
            struct.pack_into("<I", chks, i * 0x20 + 0x1C, rng.choice((0, 0, 1, -1, rng.choice(INTS))) & 0xFFFFFFFF)
    pairs = bytearray(MAX_PAIRS * 8)
    for i in range(MAX_PAIRS):
        if i < npair:
            struct.pack_into("<f", pairs, i * 8, rng.choice((1.0, 3.16, 100.0, 113.6, _float(rng))))
            struct.pack_into("<I", pairs, i * 8 + 4, rng.choice(INTS) & 0xFFFFFFFF)
    writes = [(heap + H_BANK, bytes(bank)), (heap + H_SUBS, bytes(subs)), (heap + H_CHKS, bytes(chks)),
              (heap + H_PAIRS, bytes(pairs))]
    return writes, (nsub, nchk, npair)


def _slots(writes, heap, rng, nsub):
    """Enlist slots: sub pointers, nulls and (rarely) a dangling index past the array."""
    slots = []
    for _ in range(4):
        r = rng.random()
        if r < 0.6 and nsub:
            slots.append(heap + H_SUBS + rng.randrange(nsub) * 0x2C)
        elif r < 0.7 and nsub:
            slots.append(heap + H_SUBS + nsub * 0x2C)
        else:
            slots.append(0)
    bank = bytearray(writes[0][1])
    struct.pack_into("<4I", bank, 0x1C, *slots)
    writes[0] = (writes[0][0], bytes(bank))


def _lead(writes, heap, rng, nsub, nchk):
    """The +0x34 lead: null, a sub pointer or a check pointer."""
    r = rng.random()
    if r < 0.4 or (not nsub and not nchk):
        lead = 0
    elif r < 0.7 and nsub:
        lead = heap + H_SUBS + rng.randrange(nsub) * 0x2C
    elif nchk:
        lead = heap + H_CHKS + rng.randrange(nchk) * 0x20
    elif nsub:
        lead = heap + H_SUBS + rng.randrange(nsub) * 0x2C
    else:
        lead = 0
    bank = bytearray(writes[0][1])
    struct.pack_into("<I", bank, 0x34, lead)
    writes[0] = (writes[0][0], bytes(bank))
    if lead and heap + H_SUBS <= lead < heap + H_CHKS and rng.random() < 0.8:
        # An armed, unlatched lead with thresholds a sample can land inside, between or beyond.
        hi = rng.uniform(-5.0, 120.0)
        for i, (at, data) in enumerate(writes):
            if at == heap + H_SUBS:
                sub = bytearray(data)
                off = lead - at
                struct.pack_into("<2f", sub, off + 0x14, hi, hi - rng.choice((0.5, 5.0, 10.0, 20.0, 40.0)))
                struct.pack_into("<I", sub, off + 0x1C, 0)
                writes[i] = (at, bytes(sub))


def _globals(heap, rng, mode=None, substate=None, lesson=None, cb4=None, app=None, host_time=None, base=None):
    """The heap gameflow/app/message/signal/animset objects and the pointer writes selecting them."""
    gameflow = bytearray(0xD00)
    struct.pack_into("<I", gameflow, 0, (rng.choice((7, 7, 7, 0, 1, 6, 8, rng.choice(INTS))) if mode is None else mode) & 0xFFFFFFFF)
    struct.pack_into("<I", gameflow, 0xCA4, (rng.choice((0, 2, 4, 10, 0x10, rng.randrange(17))) if lesson is None
                     else lesson) & 0xFFFFFFFF)
    struct.pack_into("<I", gameflow, 0xCA8, (rng.choice((0, 1, 2, 3, 4, 6, 7, 10)) if substate is None else substate) & 0xFFFFFFFF)
    struct.pack_into("<I", gameflow, 0xCB4, (rng.choice((0, 0, 1, rng.choice(INTS))) if cb4 is None else cb4) & 0xFFFFFFFF)
    struct.pack_into("<I", gameflow, 0xC98, heap + H_HOST)
    app_obj = bytearray(0x800)
    struct.pack_into("<I", app_obj, 0x728, rng.choice((1, 1, 8, 8, 0, 2, 10)) if app is None else app)
    msg = bytearray(0x200)
    struct.pack_into("<I", msg, 0x140, rng.getrandbits(32) if host_time is None else host_time)
    host = bytearray(0x9D00)
    struct.pack_into("<h", host, 0, 0x7B)
    mgr = bytearray(0xB000)
    struct.pack_into("<I", mgr, 0, 0)
    for off in (0x7970, 0x8A30, 0xA998):
        struct.pack_into("<I", mgr, off + 0, 0)
        struct.pack_into("<I", mgr, off + 4, heap + H_COUNT)
    anim = bytearray(0x40)
    struct.pack_into("<I", anim, 0x3C, rng.getrandbits(32) if base is None else base)
    return [(heap + H_GAMEFLOW, bytes(gameflow)), (heap + H_APP, bytes(app_obj)), (heap + H_MSG, bytes(msg)),
            (heap + H_HOST, bytes(host)), (heap + H_MGR, bytes(mgr)), (heap + H_COUNT, struct.pack("<I", 0)),
            (heap + H_ANIM, bytes(anim)), (GAMEFLOW_PTR, struct.pack("<I", heap + H_GAMEFLOW)),
            (APP_PTR, struct.pack("<I", heap + H_APP)), (MSG_PTR, struct.pack("<I", heap + H_MSG)),
            (SIGNALS_PTR, struct.pack("<I", heap + H_MGR)), (ANIM_PTR, struct.pack("<I", heap + H_ANIM))]


def _gate(heap, rng):
    """The class-gate host: +0x10 != 2 returns 1; the deep path covers the table hit/miss and the count test."""
    gate = bytearray(0x18C000)
    deep = rng.random() < 0.5
    struct.pack_into("<i", gate, 0x10, 2 if deep else rng.choice((0, 1, 3, 5)))
    idx = rng.randrange(4)
    struct.pack_into("<I", gate, 0x18B864, idx)
    struct.pack_into("<H", gate, 0x18B868, rng.choice((0, 1, 5, 0xFFFF, rng.getrandbits(16))))
    for i in range(4):
        struct.pack_into("<H", gate, 0x44 + i * 4, rng.choice((0, 3, 10, 0xFFFF)))
    struct.pack_into("<I", gate, 0x18B86C, rng.choice((0, 1, 5, rng.getrandbits(32))))
    struct.pack_into("<I", gate, 0x40, rng.choice((0, 1, 5, rng.getrandbits(32))))
    return [(heap + H_GOP, bytes(gate)), (GATE_PTR, struct.pack("<I", heap + H_GOP))]


def _parts(heap, rng):
    """The repair handler: SetHandler lands a nonzero handler, every part answers GetDamage from +0x20."""
    damage = rng.choice((0, 1, 2, 3, 4, 5, 6, 7, 100, 0xFFFFFFFF, rng.getrandbits(32)))
    handler = bytearray(0x408)
    struct.pack_into("<I", handler, 4, 1)
    parts = bytearray(256 * 0x24)
    for i in range(256):
        at = i * 0x24
        d = damage if rng.random() < 0.9 else rng.choice((0, 1, 2, 3, 4, 5, 6, 100))
        struct.pack_into("<I", parts, at + 0, heap + H_VT)
        struct.pack_into("<I", parts, at + 0x20, d)
        struct.pack_into("<I", handler, 8 + i * 4, heap + H_PARTS + at)
    # The part Vf00: SeverityMax pushes one word per part, GetDamage's `jmp` (not `call`) hands that word to
    # Vf00, and the epilogue pops once, so Vf00 pops its own argument: `mov 0x20(%ecx),%eax; ret 4`.
    return [(heap + H_VT, struct.pack("<I", heap + H_VF00)),
            (heap + H_VF00, bytes((0x8B, 0x41, 0x20, 0xC2, 0x04, 0x00))),
            (heap + H_HANDLER, bytes(handler)), (heap + H_PARTS, bytes(parts))]


def _heap(args):
    return args[0] - H_BANK


def _bank_nodes(mu, args):
    # The 0x60 bank itself is already the spec's window 0; these follow it.
    heap = _heap(args)
    return [(heap + H_SUBS, MAX_SUBS * 0x2C), (heap + H_CHKS, MAX_CHKS * 0x20),
            (heap + H_PAIRS, MAX_PAIRS * 8)]


def _gf_nodes(mu, args):
    heap = _heap(args)
    return [(heap + H_GAMEFLOW, 0xD00), (heap + H_APP + 0x728, 4), (heap + H_MSG + 0x140, 4)]


def _full_nodes(mu, args):
    return _bank_nodes(mu, args) + _gf_nodes(mu, args)


def _sub_call(*kinds):
    """A root call on a fuzzed sub at H_BANK: stack words by kind ("f" float, "i" int)."""
    def fuzz(mu, heap, rng):
        del mu
        words = [fbits(_float(rng)) if k == "f" else rng.choice(INTS) & 0xFFFFFFFF for k in kinds]
        return words, [(heap + H_BANK, struct.pack("<11I", *_sub_words(rng)))], heap + H_BANK
    return fuzz


def _sub_nodes(mu, args):
    return [(args[0], 0x2C)]


def _init_state_fuzz(mu, heap, rng):
    del mu
    return [], [(heap + H_BANK, bytes(rng.getrandbits(8) for _ in range(0x60)))], heap + H_BANK


def _tick_fuzz(mu, heap, rng):
    writes, (nsub, _, _) = _bank_writes(heap, rng)
    _slots(writes, heap, rng, nsub)
    vec = struct.pack("<3f", _float(rng), _float(rng), _float(rng))
    writes.append((heap + H_VEC, vec))
    writes.extend(_globals(heap, rng))
    return [heap + H_VEC], writes, heap + H_BANK


def _tick_nodes(mu, args):
    return _bank_nodes(mu, args) + [(args[1], 12)] + _gf_nodes(mu, args)


def _over_fuzz(mu, heap, rng):
    del mu
    words = [heap + H_BANK + 0x40]
    writes = [(heap + H_BANK, struct.pack("<11I", *_sub_words(rng))),
              (heap + H_BANK + 0x40, struct.pack("<11I", *_sub_words(rng)))]
    return words, writes, heap + H_BANK


def _bank_call(*kinds):
    """A root call on a fuzzed bank: stack words by kind."""
    def fuzz(mu, heap, rng):
        del mu
        writes, _ = _bank_writes(heap, rng)
        writes.extend(_globals(heap, rng))
        words = [fbits(_float(rng)) if k == "f" else rng.choice(INTS) & 0xFFFFFFFF for k in kinds]
        return words, writes, heap + H_BANK
    return fuzz


def _incident_fuzz(mu, heap, rng):
    writes, (nsub, nchk, _) = _bank_writes(heap, rng)
    _slots(writes, heap, rng, nsub)
    _lead(writes, heap, rng, nsub, nchk)
    drive = word(mu, DRIVE_PTR)
    count = word(mu, drive + 0x75310) if drive else 0
    spread = max(count - 1, 1)
    origin = rng.choice((0.0, rng.uniform(0, spread), spread + 5.0, -3.0, _float(rng)))
    bank = bytearray(writes[0][1])
    struct.pack_into("<f", bank, 0, origin)
    writes[0] = (writes[0][0], bytes(bank))
    vec = bytearray(0x40)
    if rng.random() < 0.7:  # the speed (a vector length scaled by 3.6) lands where the lead thresholds are
        struct.pack_into("<3f", vec, 0x30, *(rng.uniform(-20.0, 20.0) for _ in range(3)))
    else:
        struct.pack_into("<3f", vec, 0x30, _float(rng), _float(rng), _float(rng))
    struct.pack_into("<3f", vec, 0x10, _float(rng), _float(rng), _float(rng))
    writes.append((heap + H_VEC, bytes(vec)))
    if rng.random() < 0.8:
        # Past the mode, app and state gates most of the time, so the lead tests are reached.
        writes.extend(_globals(heap, rng, mode=7, app=rng.choice((1, 8)), cb4=rng.choice((0, 1))))
    else:
        writes.extend(_globals(heap, rng))
    writes.extend(_gate(heap, rng))
    return [heap + H_VEC], writes, heap + H_BANK


def _incident_nodes(mu, args):
    return _bank_nodes(mu, args) + [(args[1], 0x40)] + _gf_nodes(mu, args)


def _sample_after(mu, args, eax):
    del eax
    return [(args[2], 16)]


def _checkpoint_fuzz(mu, heap, rng):
    writes, _ = _bank_writes(heap, rng, nsub=0, nchk=0, npair=0)
    ckpt = bytearray(0x20)
    ckpt[0x18] = rng.choice((0, 1, 2, 3, 4, 5, 6, 7, 0xFF, rng.randrange(256)))
    writes.append((heap + H_CKPT, bytes(ckpt)))
    writes.extend(_globals(heap, rng))
    return [heap + H_CKPT], writes, heap + H_BANK


def _checkpoint_nodes(mu, args):
    return _bank_nodes(mu, args) + [(args[1], 0x20)] + _gf_nodes(mu, args)


def _endrace_fuzz(mu, heap, rng):
    writes, _ = _bank_writes(heap, rng)
    writes.extend(_globals(heap, rng))
    return [rng.choice(INTS) & 0xFFFFFFFF, fbits(_float(rng))], writes, heap + H_BANK


def _endrace_nodes(mu, args):
    return _bank_nodes(mu, args) + _gf_nodes(mu, args)


def _trigger_fuzz(mu, heap, rng):
    writes, _ = _bank_writes(heap, rng)
    writes.extend(_globals(heap, rng))
    return [fbits(_float(rng))], writes, heap + H_BANK


def _tracker_fuzz(mu, heap, rng):
    writes, _ = _bank_writes(heap, rng)
    writes.extend(_globals(heap, rng))
    choices = (0.0, 1.0, 44.0, 100.0, 102.0, 113.314, -5.0, _float(rng), rng.getrandbits(32))
    p = rng.choice(choices)
    pbits = fbits(p) if isinstance(p, float) else p & 0xFFFFFFFF
    writes.append((heap + H_VEC, struct.pack("<I", pbits)))
    return [heap + H_VEC], writes, heap + H_BANK


def _tracker_nodes(mu, args):
    return _endrace_nodes(mu, args) + [(args[1], 4)]


def _publish_fuzz(mu, heap, rng):
    writes, (nsub, nchk, _) = _bank_writes(heap, rng, nsub=rng.randrange(0, 5), nchk=rng.randrange(0, 5))
    # The BSS block at 0x893604: origin +0x00, signal flags +0x04, button mask +0x08, check bytes +0x0c,
    # HUD time +0x1c, button bytes +0x20.
    mirr = bytearray(48)
    for i in range(16):
        mirr[0x20 + i] = rng.choice((0, 7, 0xFF, rng.randrange(256)))
        mirr[0x0C + i] = rng.choice((0, 0, 1, 0xFF, rng.randrange(256)))
    struct.pack_into("<I", mirr, 0x00, rng.getrandbits(32))
    struct.pack_into("<I", mirr, 0x04, rng.choice((0, 1, 2, 3)))
    struct.pack_into("<I", mirr, 0x08, rng.getrandbits(32))
    struct.pack_into("<I", mirr, 0x1C, rng.getrandbits(32))
    # The original reads the real BSS directly (no pointer to redirect), so the fuzz lands there; the
    # machine is fresh per call.
    writes.append((0x893604, bytes(mirr)))
    writes.extend(_globals(heap, rng))
    return [], writes, heap + H_BANK


def _publish_nodes(mu, args):
    return _bank_nodes(mu, args) + [(0x893604, 48)] + _gf_nodes(mu, args)


def _eval_fuzz(mu, heap, rng):
    writes, _ = _bank_writes(heap, rng)
    bank_at, bank = writes[0]
    bank = bytearray(bank)
    if rng.random() < 0.5:
        # Unlatched: the verdict runs the damage loop; latched returns 4 at once.
        struct.pack_into("<I", bank, 0x44, 0)
        struct.pack_into("<I", bank, 0x48, 0)
    struct.pack_into("<I", bank, 0x5C, heap + H_HANDLER)
    writes[0] = (bank_at, bytes(bank))
    writes.extend(_globals(heap, rng, lesson=rng.choice((0, 2, 4, 6, 10, 0x10, rng.randrange(17)))))
    writes.extend(_parts(heap, rng))
    v = rng.choice((0.0, 8.5, 9.0, 9.5, 10.0, 10.5, 11.0, 12.0, _float(rng)))
    return [fbits(v)], writes, heap + H_BANK


def _severity_fuzz(mu, heap, rng):
    writes, _ = _bank_writes(heap, rng, nsub=0, nchk=0, npair=0)
    bank_at, bank = writes[0]
    bank = bytearray(bank)
    struct.pack_into("<I", bank, 0x5C, heap + H_HANDLER)
    writes[0] = (bank_at, bytes(bank))
    writes.extend(_globals(heap, rng, lesson=rng.choice((0, 2, 4, 6, 10, 0x10, rng.randrange(17)))))
    writes.extend(_parts(heap, rng))
    return [], writes, heap + H_BANK


def _stream_fuzz(mu, heap, rng):
    magic = rng.choice((6, 6, 6, 5, 0, 7, rng.getrandbits(32)))
    # One record in ten is an empty lesson: it takes the nsub/nchk/npair skip jumps and the all-empty
    # path through the handler setup.
    empty = magic == 6 and rng.random() < 0.1
    header = [magic, 0 if empty else rng.randrange(0, MAX_SUBS + 1), 0 if empty else rng.randrange(0, MAX_CHKS + 1),
              0 if empty else rng.randrange(0, MAX_PAIRS + 1),
              fbits(rng.choice((0.0, 9.0, 80.0, _float(rng)))), fbits(rng.choice((0.0, 10.0, 84.0, _float(rng)))),
              fbits(rng.choice((0.0, 11.0, 88.0, _float(rng))))]
    stream = bytearray(struct.pack("<7I", *header))
    for _ in range(MAX_SUBS):
        stream.extend(struct.pack("<10I", *[(fbits(_float(rng)) if i > 1 else rng.getrandbits(32) & 0xFFFFFFFF)
                                            if rng.random() < 0.5 else _sub_words(rng)[i] for i in range(10)]))
    for _ in range(MAX_CHKS):
        f = [_float(rng) for _ in range(6)]
        if rng.random() < 0.5:
            f[4] = 0.0
        if rng.random() < 0.5:
            f[5] = 0.0
        stream.extend(struct.pack("<6f", *f))
    for _ in range(MAX_PAIRS):
        stream.extend(struct.pack("<f", _float(rng)))
        stream.extend(struct.pack("<I", rng.choice(INTS) & 0xFFFFFFFF))
    reader = struct.pack("<3I", 0, heap + H_STREAM, 0)
    writes = [(heap + H_STREAM, bytes(stream)), (heap + H_READER, reader),
              (heap + H_BANK, bytes(0x60)), (heap + H_SUBS, bytes(MAX_SUBS * 0x2C)),
              (heap + H_CHKS, bytes(MAX_CHKS * 0x20)), (heap + H_PAIRS, bytes(MAX_PAIRS * 8))]
    writes.extend(_globals(heap, rng))
    return [heap + H_READER], writes, heap + H_BANK


def _stream_write_len(mu, heap):
    magic, nsub, nchk, npair = struct.unpack("<4I", mu.mem_read(heap + H_STREAM, 16))
    if magic != 6:
        return 28
    return 28 + 40 * nsub + 24 * nchk + 8 * npair


def _stream_nodes(mu, args):
    # k=1 reader, k=2 stream, k=3 block header, k=4 anim base, then the gameflow/app/message nodes.
    heap = args[1] - H_READER
    stream_len = _stream_write_len(mu, heap)
    return [(args[1], 12), (word(mu, args[1] + 4), stream_len), (heap + H_ANIM, 0x24),
            (heap + H_ANIM + 0x3C, 4)] + _gf_nodes(mu, args)


def _stream_after(mu, args, eax):
    del eax
    bank = args[0]
    out = []
    for off, count_off, stride in ((0x18, 0x14, 0x2C), (0x30, 0x2C, 0x20), (0x3C, 0x38, 8)):
        at, n = word(mu, bank + off), word(mu, bank + count_off)
        if at and n:
            out.append((at, n * stride))
    return out


SPECS = {
    0x497B10: dict(name="E19MaxAbs", popped=0, x87=True, thiscall=True, direct_fuzz=_sub_call(),
                   windows=[(0, lambda a: 12)]),
    0x497CF0: dict(name="E19FltSub::InRange", popped=4, x87=False, thiscall=True, direct_fuzz=_sub_call("f"),
                   windows=[(0, lambda a: 0x2C)]),
    0x497D20: dict(name="E19FltSub::Init", popped=12, x87=False, thiscall=True, direct_fuzz=_sub_call("f", "f", "f"),
                   windows=[(0, lambda a: 0x2C)]),
    0x497D70: dict(name="E19FltSub::Deref", popped=0, x87=True, thiscall=True, direct_fuzz=_sub_call(),
                   windows=[(0, lambda a: 0x2C)]),
    0x497D80: dict(name="E19FltSub::SetThresh", popped=12, x87=False, thiscall=True, direct_fuzz=_sub_call("i", "i", "i"),
                   windows=[(0, lambda a: 0x2C)]),
    0x497DA0: dict(name="E19FltSub::IsArmed", popped=0, x87=False, thiscall=True, direct_fuzz=_sub_call(),
                   windows=[(0, lambda a: 0x2C)]),
    0x497DD0: dict(name="E19FltSub::Trip", popped=4, x87=False, thiscall=True, direct_fuzz=_sub_call("f"),
                   windows=[(0, lambda a: 0x2C)]),
    0x497E20: dict(name="E19FltSub::SetLim", popped=12, x87=False, thiscall=True, direct_fuzz=_sub_call("i", "i", "i"),
                   windows=[(0, lambda a: 0x2C)]),
    0x497E40: dict(name="E19FltSub::Get20", popped=0, x87=False, thiscall=True, direct_fuzz=_sub_call(),
                   windows=[(0, lambda a: 0x2C)]),
    0x497E50: dict(name="E19FltSub::Get24", popped=0, x87=False, thiscall=True, direct_fuzz=_sub_call(),
                   windows=[(0, lambda a: 0x2C)]),
    0x497E60: dict(name="E19FltSub::Get28", popped=0, x87=False, thiscall=True, direct_fuzz=_sub_call(),
                   windows=[(0, lambda a: 0x2C)]),
    0x497E70: dict(name="E19FltSub::Classify", popped=4, x87=False, thiscall=True, direct_fuzz=_sub_call("f"),
                   windows=[(0, lambda a: 0x2C)]),
    0x497F70: dict(name="E19FltSub::Pick", popped=0, x87=True, thiscall=True, direct_fuzz=_sub_call(),
                   windows=[(0, lambda a: 0x2C)]),
    0x497F90: dict(name="E19FltSub::WindowHit", popped=4, x87=False, thiscall=True, direct_fuzz=_sub_call("f"),
                   windows=[(0, lambda a: 0x2C)]),
    0x498000: dict(name="E19FltSub::Set7", popped=24, x87=False, thiscall=True, direct_fuzz=_sub_call("i", "i", "f", "f", "f", "f"),
                   windows=[(0, lambda a: 0x2C)]),
    0x498040: dict(name="E19FltSub::MaxStore", popped=8, x87=False, thiscall=True, direct_fuzz=_sub_call("i", "i"),
                   windows=[(0, lambda a: 0x2C)]),
    0x498060: dict(name="E19FltSub::Over", popped=4, x87=False, thiscall=True, direct_fuzz=_sub_call("f"),
                   windows=[(0, lambda a: 0x2C)]),
    0x4980A0: dict(name="E19FltSub::FlagMap", popped=0, x87=False, thiscall=True, direct_fuzz=_sub_call(),
                   windows=[(0, lambda a: 0x2C)]),
    0x4981A0: dict(name="E19FltSub::IsZeroAny", popped=0, x87=False, thiscall=True, direct_fuzz=_sub_call(),
                   windows=[(0, lambda a: 0x2C)]),
    0x4981E0: dict(name="E19FltSub::Category", popped=4, x87=False, thiscall=True, direct_fuzz=_sub_call("f"),
                   windows=[(0, lambda a: 0x2C)]),
    0x4983F0: dict(name="E19FltSub::Get0c", popped=0, x87=True, thiscall=True, direct_fuzz=_sub_call(),
                   windows=[(0, lambda a: 0x2C)]),
    0x498400: dict(name="E19FltSub::Get08", popped=0, x87=True, thiscall=True, direct_fuzz=_sub_call(),
                   windows=[(0, lambda a: 0x2C)]),
    0x498410: dict(name="E19FltSub::Get10", popped=0, x87=True, thiscall=True, direct_fuzz=_sub_call(),
                   windows=[(0, lambda a: 0x2C)]),
    0x498420: dict(name="E19FltSub::Over2", popped=4, x87=False, thiscall=True, direct_fuzz=_over_fuzz,
                   windows=[(0, lambda a: 0x2C), (1, lambda a: 0x2C)]),
    0x498450: dict(name="E19FltSub::Over3", popped=4, x87=False, thiscall=True, direct_fuzz=_over_fuzz,
                   windows=[(0, lambda a: 0x2C), (1, lambda a: 0x2C)]),
    0x498630: dict(name="E19Chk::Clear", popped=0, x87=False, thiscall=True, direct_fuzz=_sub_call(),
                   windows=[(0, lambda a: 0x20)]),
    0x498660: dict(name="E19FltSubInit", popped=0, x87=False, thiscall=True, direct_fuzz=_sub_call(),
                   windows=[(0, lambda a: 0x2C)]),
    0x4986A0: dict(name="E19FltSub::Update", popped=4, x87=False, thiscall=True, direct_fuzz=_sub_call("f"),
                   windows=[(0, lambda a: 0x2C)]),
    0x4980C0: dict(name="CReplayManager::InitState", popped=0, x87=False, thiscall=True,
                   direct_fuzz=_init_state_fuzz, windows=[(0, lambda a: 0x60)], nodes=_bank_nodes),
    0x498240: dict(name="CReplayManager::SeverityMax", popped=0, x87=False, thiscall=True,
                   direct_fuzz=_severity_fuzz, windows=[(0, lambda a: 0x60)], nodes=_full_nodes),
    0x4982E0: dict(name="CReplayManager::EvalFilters", popped=4, x87=False, thiscall=True, direct_fuzz=_eval_fuzz,
                   windows=[(0, lambda a: 0x60)], nodes=_full_nodes),
    0x4984A0: dict(name="CReplayManager::Get44", popped=0, x87=False, thiscall=True, direct_fuzz=_bank_call(),
                   windows=[(0, lambda a: 0x60)], nodes=_bank_nodes),
    0x4984B0: dict(name="CReplayManager::Get48", popped=0, x87=False, thiscall=True, direct_fuzz=_bank_call(),
                   windows=[(0, lambda a: 0x60)], nodes=_bank_nodes),
    0x4984C0: dict(name="CReplayManager::Set44", popped=4, x87=False, thiscall=True, direct_fuzz=_bank_call("i"),
                   windows=[(0, lambda a: 0x60)], nodes=_bank_nodes),
    0x4984D0: dict(name="CReplayManager::Set48", popped=4, x87=False, thiscall=True, direct_fuzz=_bank_call("i"),
                   windows=[(0, lambda a: 0x60)], nodes=_bank_nodes),
    0x4984E0: dict(name="CReplayManager::Set48_1", popped=0, x87=False, thiscall=True, direct_fuzz=_bank_call(),
                   windows=[(0, lambda a: 0x60)], nodes=_bank_nodes),
    0x498500: dict(name="CReplayManager::Set44_1", popped=0, x87=False, thiscall=True, direct_fuzz=_bank_call(),
                   windows=[(0, lambda a: 0x60)], nodes=_bank_nodes),
    0x498510: dict(name="CReplayManager::Clear40", popped=0, x87=False, thiscall=True, direct_fuzz=_bank_call(),
                   windows=[(0, lambda a: 0x60)], nodes=_bank_nodes),
    0x498520: dict(name="CReplayManager::Publish", popped=0, x87=False, thiscall=True, direct_fuzz=_publish_fuzz,
                   windows=[(0, lambda a: 0x60)], nodes=_publish_nodes),
    0x498840: dict(name="CReplayManager::ReadStream", popped=4, x87=False, thiscall=True, direct_fuzz=_stream_fuzz,
                   windows=[(0, lambda a: 0x60)], nodes=_stream_nodes, after=_stream_after),
    0x498BB0: dict(name="CReplayManager::SchoolTick", popped=4, x87=False, thiscall=True, direct_fuzz=_tick_fuzz,
                   windows=[(0, lambda a: 0x60)], nodes=_tick_nodes),
    0x498E10: dict(name="CReplayManager::SchoolIncident", popped=4, x87=False, thiscall=True,
                   direct_fuzz=_incident_fuzz, windows=[(0, lambda a: 0x60)], nodes=_incident_nodes),
    0x499110: dict(name="CReplayManager::EndRace", popped=4, stack_words=2, x87=False, thiscall=True, direct_fuzz=_endrace_fuzz,
                   windows=[(0, lambda a: 0x60)], nodes=_endrace_nodes),
    0x4991C0: dict(name="CReplayManager::Checkpoint", popped=4, x87=False, thiscall=True,
                   direct_fuzz=_checkpoint_fuzz, windows=[(0, lambda a: 0x60)], nodes=_checkpoint_nodes),
    0x499240: dict(name="CReplayManager::RallySchoolSound", popped=0, x87=False, thiscall=True,
                   direct_fuzz=_bank_call(), windows=[(0, lambda a: 0x60)], nodes=_full_nodes),
    0x4992A0: dict(name="CReplayManager::Trigger", popped=4, x87=False, thiscall=True, direct_fuzz=_trigger_fuzz,
                   windows=[(0, lambda a: 0x60)], nodes=_endrace_nodes),
    0x499360: dict(name="CReplayManager::Tracker", popped=4, x87=False, thiscall=True, direct_fuzz=_tracker_fuzz,
                   windows=[(0, lambda a: 0x60)], nodes=_tracker_nodes),
    # Externals: emits carry the event payloads, the gate/sample/damage calls carry the typed inputs.
    0x498C70: dict(name="SoundRallySchool::Emit", popped=4, x87=False, thiscall=True, external=True,
                   windows=[(1, lambda a: 16)]),
    0x498D30: dict(name="SchoolIncident::Emit", popped=4, x87=False, thiscall=True, external=True,
                   windows=[(1, lambda a: 4)]),
    0x477920: dict(name="EndRace::Emit", popped=4, x87=False, thiscall=True, external=True,
                   windows=[(1, lambda a: 20)]),
    0x49A0F0: dict(name="E19ClsGate", popped=0, x87=False, thiscall=True, external=True, windows=[]),
    0x50B180: dict(name="DriveLine::Sample", popped=8, x87=False, thiscall=True, external=True, windows=[],
                   after=_sample_after),
    0x4F2B40: dict(name="RepairHandler::GetDamage", popped=4, x87=False, thiscall=True, external=True, windows=[]),
    0x4F2BA0: dict(name="RepairHandler::SetHandler", popped=4, x87=False, thiscall=True, external=True,
                   windows=[]),
    0x565F90: dict(name="M00565f90", popped=0, x87=False, thiscall=True, external=True, windows=[]),
}
