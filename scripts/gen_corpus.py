"""EXL benchmark corpus generator.
Produces STL (ASCII/binary), OBJ, and STEP (ISO-10303-21) test models
into the corpus/ directory.  Deterministic — seeded with a constant.
"""

import math
import os
import struct
import random

CORPUS = os.path.join(os.path.dirname(__file__), "..", "corpus")

random.seed(42)


def _write(path, content):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    if isinstance(content, str):
        with open(path, "w", newline="\n") as f:
            f.write(content)
    else:
        with open(path, "wb") as f:
            f.write(content)


def _path(name):
    return os.path.join(CORPUS, name)


# ── geometry builders ────────────────────────────────────────────────────────


def _normal(a, b, c):
    ux, uy, uz = b[0] - a[0], b[1] - a[1], b[2] - a[2]
    vx, vy, vz = c[0] - a[0], c[1] - a[1], c[2] - a[2]
    nx = uy * vz - uz * vy
    ny = uz * vx - ux * vz
    nz = ux * vy - uy * vx
    length = math.sqrt(nx * nx + ny * ny + nz * nz)
    if length < 1e-12:
        return (0.0, 0.0, 0.0)
    return (nx / length, ny / length, nz / length)


def _add(v1, v2):
    return (v1[0] + v2[0], v1[1] + v2[1], v1[2] + v2[2])


def _sub(v1, v2):
    return (v1[0] - v2[0], v1[1] - v2[1], v1[2] - v2[2])


def _scale(v, s):
    return (v[0] * s, v[1] * s, v[2] * s)


def _normalize(v):
    length = math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2])
    if length < 1e-12:
        return (0.0, 0.0, 0.0)
    return (v[0] / length, v[1] / length, v[2] / length)


def _cross(a, b):
    return (
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    )


def _dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def _midpoint(a, b):
    return ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2, (a[2] + b[2]) / 2)


def _rot_x(v, angle):
    c, s = math.cos(angle), math.sin(angle)
    return (v[0], v[1] * c - v[2] * s, v[1] * s + v[2] * c)


def _rot_y(v, angle):
    c, s = math.cos(angle), math.sin(angle)
    return (v[0] * c + v[2] * s, v[1], -v[0] * s + v[2] * c)


def _rot_z(v, angle):
    c, s = math.cos(angle), math.sin(angle)
    return (v[0] * c - v[1] * s, v[0] * s + v[1] * c, v[2])


def box_geometry(sx=1.0, sy=1.0, sz=1.0, cx=0.0, cy=0.0, cz=0.0):
    hx, hy, hz = sx / 2.0, sy / 2.0, sz / 2.0
    v = [
        (cx - hx, cy - hy, cz - hz),
        (cx + hx, cy - hy, cz - hz),
        (cx + hx, cy + hy, cz - hz),
        (cx - hx, cy + hy, cz - hz),
        (cx - hx, cy - hy, cz + hz),
        (cx + hx, cy - hy, cz + hz),
        (cx + hx, cy + hy, cz + hz),
        (cx - hx, cy + hy, cz + hz),
    ]
    faces = [
        (0, 1, 2, 3),
        (4, 7, 6, 5),
        (0, 4, 5, 1),
        (1, 5, 6, 2),
        (2, 6, 7, 3),
        (3, 7, 4, 0),
    ]
    tris = []
    for f in faces:
        a, b, c, d = v[f[0]], v[f[1]], v[f[2]], v[f[3]]
        tris.append((a, b, c))
        tris.append((a, c, d))
    return tris


def _icosahedron_vertices():
    phi = (1.0 + math.sqrt(5.0)) / 2.0
    verts = [
        (-1, phi, 0), (1, phi, 0), (-1, -phi, 0), (1, -phi, 0),
        (0, -1, phi), (0, 1, phi), (0, -1, -phi), (0, 1, -phi),
        (phi, 0, -1), (phi, 0, 1), (-phi, 0, -1), (-phi, 0, 1),
    ]
    return [_normalize(v) for v in verts]


def icosphere_geometry(subdivisions):
    verts = _icosahedron_vertices()
    faces = [
        (0, 11, 5), (0, 5, 1), (0, 1, 7), (0, 7, 10), (0, 10, 11),
        (1, 5, 9), (5, 11, 4), (11, 10, 2), (10, 7, 6), (7, 1, 8),
        (3, 9, 4), (3, 4, 2), (3, 2, 6), (3, 6, 8), (3, 8, 9),
        (4, 9, 5), (2, 4, 11), (6, 2, 10), (8, 6, 7), (9, 8, 1),
    ]
    verts = list(verts)
    mid_cache = {}
    for _ in range(subdivisions):
        new_faces = []
        for tri in faces:
            v0, v1, v2 = verts[tri[0]], verts[tri[1]], verts[tri[2]]
            a_key = tuple(sorted((tri[0], tri[1])))
            b_key = tuple(sorted((tri[1], tri[2])))
            c_key = tuple(sorted((tri[2], tri[0])))
            if a_key not in mid_cache:
                mid_cache[a_key] = len(verts)
                verts.append(_normalize(_midpoint(v0, v1)))
            if b_key not in mid_cache:
                mid_cache[b_key] = len(verts)
                verts.append(_normalize(_midpoint(v1, v2)))
            if c_key not in mid_cache:
                mid_cache[c_key] = len(verts)
                verts.append(_normalize(_midpoint(v2, v0)))
            a, b, c = mid_cache[a_key], mid_cache[b_key], mid_cache[c_key]
            new_faces.extend([
                (tri[0], a, c),
                (tri[1], b, a),
                (tri[2], c, b),
                (a, b, c),
            ])
        faces = new_faces
    tris = [(verts[f[0]], verts[f[1]], verts[f[2]]) for f in faces]
    radius = 1.0
    return tris, radius


def cylinder_geometry(radius=1.0, height=2.0, segments=16, cx=0, cy=0, cz=0):
    verts_bot = []
    verts_top = []
    for i in range(segments):
        angle = 2.0 * math.pi * i / segments
        x = cx + radius * math.cos(angle)
        y = cy + radius * math.sin(angle)
        verts_bot.append((x, y, cz - height / 2))
        verts_top.append((x, y, cz + height / 2))
    tris = []
    for i in range(segments):
        j = (i + 1) % segments
        a, b = verts_bot[i], verts_bot[j]
        c, d = verts_top[i], verts_top[j]
        tris.append((a, b, c))
        tris.append((b, d, c))
        tris.append((verts_bot[i], (cx, cy, cz - height / 2), b))
        tris.append((c, d, (cx, cy, cz + height / 2)))
    return tris


def cone_geometry(radius=1.0, height=2.0, segments=24, cx=0, cy=0, cz=0):
    verts_bot = []
    for i in range(segments):
        angle = 2.0 * math.pi * i / segments
        x = cx + radius * math.cos(angle)
        y = cy + radius * math.sin(angle)
        verts_bot.append((x, y, cz - height / 2))
    apex = (cx, cy, cz + height / 2)
    tris = []
    for i in range(segments):
        j = (i + 1) % segments
        tris.append((verts_bot[i], verts_bot[j], (cx, cy, cz - height / 2)))
    for i in range(segments):
        j = (i + 1) % segments
        tris.append((verts_bot[j], verts_bot[i], apex))
    return tris


def torus_geometry(major_r=1.0, minor_r=0.4, u_seg=24, v_seg=12, cx=0, cy=0, cz=0):
    verts = []
    for ui in range(u_seg):
        u = 2.0 * math.pi * ui / u_seg
        for vi in range(v_seg):
            v = 2.0 * math.pi * vi / v_seg
            x = cx + (major_r + minor_r * math.cos(v)) * math.cos(u)
            y = cy + (major_r + minor_r * math.cos(v)) * math.sin(u)
            z = cz + minor_r * math.sin(v)
            verts.append((x, y, z))
    tris = []
    for ui in range(u_seg):
        unext = (ui + 1) % u_seg
        for vi in range(v_seg):
            vnext = (vi + 1) % v_seg
            a = ui * v_seg + vi
            b = ui * v_seg + vnext
            c = unext * v_seg + vi
            d = unext * v_seg + vnext
            tris.append((verts[a], verts[b], verts[d]))
            tris.append((verts[a], verts[d], verts[c]))
    return tris


def sphere_uv_geometry(radius=1.0, u_seg=24, v_seg=12, cx=0, cy=0, cz=0):
    verts = [(cx, cy, cz + radius), (cx, cy, cz - radius)]
    for vi in range(1, v_seg):
        v = math.pi * vi / v_seg
        for ui in range(u_seg):
            u = 2.0 * math.pi * ui / u_seg
            x = cx + radius * math.sin(v) * math.cos(u)
            y = cy + radius * math.sin(v) * math.sin(u)
            z = cz + radius * math.cos(v)
            verts.append((x, y, z))
    tris = []
    for ui in range(u_seg):
        unext = (ui + 1) % u_seg
        tris.append((verts[0], verts[2 + ui], verts[2 + unext]))
    for vi in range(v_seg - 2):
        row_off = 2 + vi * u_seg
        next_off = 2 + (vi + 1) * u_seg
        for ui in range(u_seg):
            unext = (ui + 1) % u_seg
            a = row_off + ui
            b = row_off + unext
            c = next_off + ui
            d = next_off + unext
            tris.append((verts[a], verts[b], verts[d]))
            tris.append((verts[a], verts[d], verts[c]))
    for ui in range(u_seg):
        unext = (ui + 1) % u_seg
        last = 2 + (v_seg - 2) * u_seg
        tris.append((verts[1], verts[last + unext], verts[last + ui]))
    return tris


def l_bracket_geometry(w=4, h=0.5, d=1, t=0.2):
    hw, hh, hd = w / 2, h / 2, d / 2
    verts = [
        (-hw, -hh, -hd), (hw, -hh, -hd), (hw, -hh, hd), (-hw, -hh, hd),
        (-hw + t, -hh, -hd + t), (hw - t, -hh, -hd + t), (hw - t, -hh, hd - t),
        (-hw + t, -hh, hd - t),
        (-hw + t, hh, -hd + t), (hw - t, hh, -hd + t), (hw - t, hh, hd - t),
        (-hw + t, hh, hd - t),
        (-hw, hh, -hd), (hw, hh, -hd), (hw, hh, hd), (-hw, hh, hd),
    ]
    faces = [
        (0, 1, 5, 4), (2, 3, 7, 6),
        (4, 5, 9, 8), (7, 11, 10, 6),
        (0, 12, 13, 1), (1, 13, 14, 2),
        (2, 14, 15, 3), (3, 15, 12, 0),
        (4, 8, 12),
        (5, 9, 13),
        (6, 10, 14),
        (7, 11, 15),
        (5, 13, 9),
        (6, 14, 10),
        (8, 12, 0, 4),
        (9, 13, 1, 5),
        (10, 14, 2, 6),
        (11, 15, 3, 7),
        (12, 8, 11, 15),
        (8, 9, 10, 11),
    ]
    tris = []
    for f in faces:
        p = [verts[i] for i in f]
        if len(p) == 3:
            tris.append((p[0], p[1], p[2]))
        elif len(p) == 4:
            tris.append((p[0], p[1], p[2]))
            tris.append((p[0], p[2], p[3]))
    return tris


def plate_geometry(w=3, d=2, t=0.1):
    hw, hd, ht = w / 2, d / 2, t / 2
    v = [
        (-hw, -ht, -hd), (hw, -ht, -hd), (hw, -ht, hd), (-hw, -ht, hd),
        (-hw, ht, -hd), (hw, ht, -hd), (hw, ht, hd), (-hw, ht, hd),
    ]
    faces = [
        (0, 1, 2, 3), (4, 7, 6, 5),
        (0, 4, 5, 1), (1, 5, 6, 2),
        (2, 6, 7, 3), (3, 7, 4, 0),
    ]
    tris = []
    for f in faces:
        p = [v[i] for i in f]
        tris.append((p[0], p[1], p[2]))
        tris.append((p[0], p[2], p[3]))
    return tris


def open_box_geometry(sx=1, sy=1, sz=1):
    v = box_geometry(sx, sy, sz)
    return v[:-2]


def degenerate_tri_geometry():
    a = (0, 0, 0)
    b = (1, 0, 0)
    c = (0.5, 0, 0)
    d = (0, 0.5, 0)
    e = (1, 0.5, 0)
    f = (0.5, 1, 0)
    return [
        (a, b, c), (d, e, f), (a, d, e), (a, e, b),
        (d, f, e), (a, b, e), (a, e, d),
    ]


# ── STL writers ──────────────────────────────────────────────────────────────


def _stl_ascii(name, tris, solid_name="solid"):
    lines = [f"solid {solid_name}"]
    for a, b, c in tris:
        n = _normal(a, b, c)
        lines.append(f"  facet normal {n[0]:.6f} {n[1]:.6f} {n[2]:.6f}")
        lines.append("    outer loop")
        lines.append(f"      vertex {a[0]:.6f} {a[1]:.6f} {a[2]:.6f}")
        lines.append(f"      vertex {b[0]:.6f} {b[1]:.6f} {b[2]:.6f}")
        lines.append(f"      vertex {c[0]:.6f} {c[1]:.6f} {c[2]:.6f}")
        lines.append("    endloop")
        lines.append("  endfacet")
    lines.append(f"endsolid {solid_name}")
    return "\n".join(lines) + "\n"


def _stl_binary(tris):
    header = b"\x00" * 80
    header = header[:80]
    count = struct.pack("<I", len(tris))
    records = b""
    for a, b, c in tris:
        n = _normal(a, b, c)
        records += struct.pack(
            "<3f 3f 3f 3f H",
            n[0], n[1], n[2],
            a[0], a[1], a[2],
            b[0], b[1], b[2],
            c[0], c[1], c[2],
            0,
        )
    return header + count + records


def _stl_crlf(tris, solid_name="solid"):
    lines = [f"solid {solid_name}"]
    for a, b, c in tris:
        n = _normal(a, b, c)
        lines.append(f"  facet normal {n[0]:.6f} {n[1]:.6f} {n[2]:.6f}")
        lines.append("    outer loop")
        lines.append(f"      vertex {a[0]:.6f} {a[1]:.6f} {a[2]:.6f}")
        lines.append(f"      vertex {b[0]:.6f} {b[1]:.6f} {b[2]:.6f}")
        lines.append(f"      vertex {c[0]:.6f} {c[1]:.6f} {c[2]:.6f}")
        lines.append("    endloop")
        lines.append("  endfacet")
    lines.append(f"endsolid {solid_name}")
    return "\r\n".join(lines) + "\r\n"


# ── OBJ writers ──────────────────────────────────────────────────────────────


def _obj_from_tris(name, tris):
    lines = [f"o {name}"]
    seen = {}
    idx_map = {}
    counter = 1
    for a, b, c in tris:
        for v in (a, b, c):
            key = (round(v[0], 8), round(v[1], 8), round(v[2], 8))
            if key not in seen:
                seen[key] = counter
                lines.append(f"v {v[0]:.6f} {v[1]:.6f} {v[2]:.6f}")
                counter += 1
        lines.append(f"f {seen[(round(a[0],8),round(a[1],8),round(a[2],8))]} "
                     f"{seen[(round(b[0],8),round(b[1],8),round(b[2],8))]} "
                     f"{seen[(round(c[0],8),round(c[1],8),round(c[2],8))]}")
    return "\n".join(lines) + "\n"


def _obj_from_tris_with_groups(name, tris, group_names):
    lines = [f"o {name}"]
    seen = {}
    counter = 1

    def _key(v):
        return (round(v[0], 8), round(v[1], 8), round(v[2], 8))

    for a, b, c in tris:
        for v in (a, b, c):
            k = _key(v)
            if k not in seen:
                seen[k] = counter
                lines.append(f"v {v[0]:.6f} {v[1]:.6f} {v[2]:.6f}")
                counter += 1
    n = len(tris)
    per = max(1, n // len(group_names))
    for gi, gn in enumerate(group_names):
        lines.append(f"g {gn}")
        start = gi * per
        end = start + per if gi < len(group_names) - 1 else n
        for a, b, c in tris[start:end]:
            ia = seen[_key(a)]
            ib = seen[_key(b)]
            ic = seen[_key(c)]
            lines.append(f"f {ia} {ib} {ic}")
    return "\n".join(lines) + "\n"


def _obj_quads_only(name, tris):
    lines = [f"o {name}"]
    seen = {}
    counter = 1

    def _key(v):
        return (round(v[0], 8), round(v[1], 8), round(v[2], 8))

    quads = []
    for i in range(0, len(tris) - 1, 2):
        t1 = tris[i]
        t2 = tris[i + 1]
        vs = [t1[0], t1[1], t1[2], t2[0], t2[1], t2[2]]
        for v in vs:
            k = _key(v)
            if k not in seen:
                seen[k] = counter
                lines.append(f"v {v[0]:.6f} {v[1]:.6f} {v[2]:.6f}")
                counter += 1
        ia = seen[_key(t1[0])]
        ib = seen[_key(t1[1])]
        ic = seen[_key(t1[2])]
        id_ = seen[_key(t2[0])]
        quads.append((ia, ib, ic, id_))
    for q in quads:
        lines.append(f"f {q[0]} {q[1]} {q[2]} {q[3]}")
    return "\n".join(lines) + "\n"


def _obj_mixed(name, tris):
    lines = [f"o {name}"]
    seen = {}
    counter = 1

    def _key(v):
        return (round(v[0], 8), round(v[1], 8), round(v[2], 8))

    for a, b, c in tris:
        for v in (a, b, c):
            k = _key(v)
            if k not in seen:
                seen[k] = counter
                lines.append(f"v {v[0]:.6f} {v[1]:.6f} {v[2]:.6f}")
                counter += 1
    for i, tri in enumerate(tris):
        a, b, c = tri
        ia = seen[_key(a)]
        ib = seen[_key(b)]
        ic = seen[_key(c)]
        if i % 3 == 0 and i + 1 < len(tris):
            nxt = tris[i + 1]
            id_ = seen[_key(nxt[0])]
            lines.append(f"f {ia} {ib} {ic} {id_}")
        else:
            lines.append(f"f {ia} {ib} {ic}")
    return "\n".join(lines) + "\n"


def _obj_negative_index(name, tris):
    lines = [f"o {name}"]
    verts = []
    seen = {}
    for a, b, c in tris:
        for v in (a, b, c):
            key = (round(v[0], 8), round(v[1], 8), round(v[2], 8))
            if key not in seen:
                seen[key] = len(verts) + 1
                verts.append(v)
                lines.append(f"v {v[0]:.6f} {v[1]:.6f} {v[2]:.6f}")
    n = len(verts)
    for a, b, c in tris:
        ia = seen[(round(a[0], 8), round(a[1], 8), round(a[2], 8))]
        ib = seen[(round(b[0], 8), round(b[1], 8), round(b[2], 8))]
        ic = seen[(round(c[0], 8), round(c[1], 8), round(c[2], 8))]
        neg_a, neg_b, neg_c = ia - n - 1, ib - n - 1, ic - n - 1
        lines.append(f"f {neg_a} {neg_b} {neg_c}")
    return "\n".join(lines) + "\n"


def _obj_multi_group_geometry():
    b1 = box_geometry(1, 1, 1, cx=-1.5)
    b2 = box_geometry(0.5, 0.5, 0.5, cx=0)
    b3 = box_geometry(0.8, 0.8, 0.8, cx=1.5)
    return b1 + b2 + b3, ["box_left", "box_center", "box_right"]


def _obj_normals_texcoords(name, tris):
    lines = [f"o {name}"]
    verts = []
    for a, b, c in tris:
        for v in (a, b, c):
            verts.append(v)
    for v in verts:
        lines.append(f"v {v[0]:.6f} {v[1]:.6f} {v[2]:.6f}")
    for i in range(0, len(verts), 3):
        a, b, c = verts[i], verts[i + 1], verts[i + 2]
        n = _normal(a, b, c)
        lines.append(f"vn {n[0]:.6f} {n[1]:.6f} {n[2]:.6f}")
    for v in verts:
        lines.append(f"vt {v[0]:.4f} {v[1]:.4f}")
    for i in range(len(tris)):
        idx = i * 3 + 1
        lines.append(f"f {idx}/{idx}/{i+1} {idx+1}/{idx+1}/{i+1} {idx+2}/{idx+2}/{i+1}")
    return "\n".join(lines) + "\n"


def _obj_usemtl(name, tris):
    lines = [
        f"o {name}",
        "mtllib test.mtl",
        "usemtl default_material",
    ]
    verts = []
    seen = {}
    for a, b, c in tris:
        for v in (a, b, c):
            key = (round(v[0], 8), round(v[1], 8), round(v[2], 8))
            if key not in seen:
                seen[key] = len(verts) + 1
                verts.append(v)
                lines.append(f"v {v[0]:.6f} {v[1]:.6f} {v[2]:.6f}")
    for a, b, c in tris:
        ia = seen[(round(a[0], 8), round(a[1], 8), round(a[2], 8))]
        ib = seen[(round(b[0], 8), round(b[1], 8), round(b[2], 8))]
        ic = seen[(round(c[0], 8), round(c[1], 8), round(c[2], 8))]
        lines.append(f"f {ia} {ib} {ic}")
    return "\n".join(lines) + "\n"


# ── STEP helpers ─────────────────────────────────────────────────────────────


class _STEPBuilder:
    def __init__(self, description="generated corpus", schema="AUTOMOTIVE_DESIGN",
                 filename="model.step", timestamp="2025-01-01T00:00:00",
                 originating_system="exl corpus generator",
                 organisation="exl"):
        self._next_id = 1
        self._entities = []
        self._point_coords = {}
        self._desc = description
        self._schema = schema
        self._fname = filename
        self._ts = timestamp
        self._org_sys = originating_system
        self._org = organisation

    def _eid(self):
        n = self._next_id
        self._next_id += 1
        return n

    def _peek(self):
        return self._next_id

    def add_point(self, x, y, z):
        n = self._eid()
        self._entities.append(f"#{n}=CARTESIAN_POINT('',({x:.6E},{y:.6E},{z:.6E}));")
        self._point_coords[n] = (x, y, z)
        return n

    def pt(self, eid):
        return self._point_coords[eid]

    def add_direction(self, dx, dy, dz):
        n = self._eid()
        self._entities.append(f"#{n}=DIRECTION('',({dx:.6E},{dy:.6E},{dz:.6E}));")
        return n

    def add_axis2_placement_3d(self, loc_id, axis_id, ref_id):
        n = self._eid()
        self._entities.append(f"#{n}=AXIS2_PLACEMENT_3D('',#{loc_id},#{axis_id},#{ref_id});")
        return n

    def add_plane(self, axis_id):
        n = self._eid()
        self._entities.append(f"#{n}=PLANE('',#{axis_id});")
        return n

    def add_cylindrical_surface(self, axis_id, radius):
        n = self._eid()
        self._entities.append(f"#{n}=CYLINDRICAL_SURFACE('',#{axis_id},{radius:.6E});")
        return n

    def add_spherical_surface(self, axis_id, radius):
        n = self._eid()
        self._entities.append(f"#{n}=SPHERICAL_SURFACE('',#{axis_id},{radius:.6E});")
        return n

    def add_conical_surface(self, axis_id, radius, semi_angle):
        n = self._eid()
        self._entities.append(f"#{n}=CONICAL_SURFACE('',#{axis_id},{radius:.6E},{semi_angle:.6E});")
        return n

    def add_toroidal_surface(self, axis_id, major_r, minor_r):
        n = self._eid()
        self._entities.append(f"#{n}=TOROIDAL_SURFACE('',#{axis_id},{major_r:.6E},{minor_r:.6E});")
        return n

    def add_vertex_point(self, point_id):
        n = self._eid()
        self._entities.append(f"#{n}=VERTEX_POINT('',#{point_id});")
        return n

    def add_line(self, point_id, direction_id):
        n = self._eid()
        self._entities.append(f"#{n}=LINE('',#{point_id},#{direction_id});")
        return n

    def add_circle(self, axis_id, radius):
        n = self._eid()
        self._entities.append(f"#{n}=CIRCLE('',#{axis_id},{radius:.6E});")
        return n

    def add_edge_curve(self, v_start, v_end, curve_id, sense=True):
        n = self._eid()
        sense_str = ".T." if sense else ".F."
        self._entities.append(f"#{n}=EDGE_CURVE('',#{v_start},#{v_end},#{curve_id},{sense_str});")
        return n

    def add_oriented_edge(self, edge_id, sense=True, edge_element=None):
        n = self._eid()
        ee = edge_element if edge_element is not None else edge_id
        s = ".T." if sense else ".F."
        self._entities.append(f"#{n}=ORIENTED_EDGE('',*,*,#{ee},{s});")
        return n

    def add_edge_loop(self, oriented_edge_ids):
        n = self._eid()
        refs = ",".join(f"#{eid}" for eid in oriented_edge_ids)
        self._entities.append(f"#{n}=EDGE_LOOP('',({refs}));")
        return n

    def add_face_outer_bound(self, loop_id, sense=True):
        n = self._eid()
        s = ".T." if sense else ".F."
        self._entities.append(f"#{n}=FACE_OUTER_BOUND('',#{loop_id},{s});")
        return n

    def add_advanced_face(self, bound_ids, surface_id, sense=True):
        n = self._eid()
        s = ".T." if sense else ".F."
        refs = ",".join(f"#{b}" for b in bound_ids)
        self._entities.append(f"#{n}=ADVANCED_FACE('',({refs}),#{surface_id},{s});")
        return n

    def add_closed_shell(self, face_ids):
        n = self._eid()
        refs = ",".join(f"#{f}" for f in face_ids)
        self._entities.append(f"#{n}=CLOSED_SHELL('',({refs}));")
        return n

    def add_manifold_solid_brep(self, shell_id, name=""):
        n = self._eid()
        nm = f"'{name}'" if name else "''"
        self._entities.append(f"#{n}=MANIFOLD_SOLID_BREP({nm},#{shell_id});")
        return n

    def add_product(self, name, description, frame_of_ref=None):
        n = self._eid()
        refs = f"(#{frame_of_ref})" if frame_of_ref else "()"
        self._entities.append(f"#{n}=PRODUCT('{name}','{description}','',{refs});")
        return n

    def add_product_definition_formation(self, prod_id, name=""):
        n = self._eid()
        self._entities.append(f"#{n}=PRODUCT_DEFINITION_FORMATION('{name}','',#{prod_id});")
        return n

    def add_product_definition(self, pdf_id, name, entities):
        n = self._eid()
        refs = ",".join(f"#{e}" for e in entities)
        self._entities.append(f"#{n}=PRODUCT_DEFINITION('{name}','',#{pdf_id},({refs}));")
        return n

    def add_shape_representation(self, items):
        n = self._eid()
        refs = ",".join(f"#{i}" for i in items)
        self._entities.append(f"#{n}=SHAPE_REPRESENTATION('',({refs}),#0);")
        return n

    def add_next_assembly_usage_occurrence(self, child_id, parent_id, loc_id):
        n = self._eid()
        self._entities.append(
            f"#{n}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('','','',"
            f"#{child_id},#{parent_id},#{loc_id});"
        )
        return n

    def add_item_defined_transformation(self, name, item1, item2):
        n = self._eid()
        self._entities.append(
            f"#{n}=ITEM_DEFINED_TRANSFORMATION('{name}','',#{item1},#{item2});"
        )
        return n

    def add_b_spline_surface_with_knots(
        self, u_degree, v_degree, control_points, u_multiplicities,
        v_multiplicities, u_knots, v_knots, u_closed=False, v_closed=False,
        self_intersect=False, surface_form="UNSPECIFIED",
    ):
        n = self._eid()
        cp_str = ",".join(
            f"({','.join(f'#{cpid}' for cpid in row)})" for row in control_points
        )
        u_m_str = ",".join(str(m) for m in u_multiplicities)
        v_m_str = ",".join(str(m) for m in v_multiplicities)
        u_k_str = ",".join(f"{k:.6E}" for k in u_knots)
        v_k_str = ",".join(f"{k:.6E}" for k in v_knots)
        u_cl = ".T." if u_closed else ".F."
        v_cl = ".T." if v_closed else ".F."
        si = ".T." if self_intersect else ".F."
        self._entities.append(
            f"#{n}=B_SPLINE_SURFACE_WITH_KNOTS("
            f"'{surface_form}',{u_degree},{v_degree},"
            f"({cp_str}),.UNSPECIFIED.,{si},{u_cl},{v_cl},"
            f"({u_m_str}),({v_m_str}),({u_k_str}),({v_k_str}));"
        )
        return n

    def add_product_definition_shape(self, pd_id, rep_id):
        n = self._eid()
        self._entities.append(
            f"#{n}=PRODUCT_DEFINITION_SHAPE('','',#{pd_id},#{rep_id});"
        )
        return n

    def build(self, header_extra=None):
        lines = ["ISO-10303-21;", "HEADER;"]
        fn = self._fname
        lines.append(f"FILE_DESCRIPTION(('{self._desc}'),'2;1');")
        lines.append(
            f"FILE_NAME('{fn}','{self._ts}',"
            f"(''),(''),'{self._org_sys}','{self._org}','');"
        )
        lines.append(f"FILE_SCHEMA(('{self._schema}'));")
        if header_extra:
            lines.extend(header_extra)
        lines.append("ENDSEC;")
        lines.append("DATA;")
        lines.extend(self._entities)
        lines.append("ENDSEC;")
        lines.append("END-ISO-10303-21;")
        return "\n".join(lines) + "\n"


# ── box face builder helper ───────────────────────────────────────────────────


def _build_box_face(b, p0_id, p1_id, p2_id, p3_id, origin_p, axis_d, ref_d):
    axis = b.add_axis2_placement_3d(origin_p, axis_d, ref_d)
    plane = b.add_plane(axis)
    v0 = b.add_vertex_point(p0_id)
    v1 = b.add_vertex_point(p1_id)
    v2 = b.add_vertex_point(p2_id)
    v3 = b.add_vertex_point(p3_id)
    c0, c1, c2, c3 = b.pt(p0_id), b.pt(p1_id), b.pt(p2_id), b.pt(p3_id)
    dirs = [
        (c1[0] - c0[0], c1[1] - c0[1], c1[2] - c0[2]),
        (c2[0] - c1[0], c2[1] - c1[1], c2[2] - c1[2]),
        (c3[0] - c2[0], c3[1] - c2[1], c3[2] - c2[2]),
        (c0[0] - c3[0], c0[1] - c3[1], c0[2] - c3[2]),
    ]
    norms = [_normalize(d) for d in dirs]
    d01 = b.add_direction(norms[0][0], norms[0][1], norms[0][2])
    d12 = b.add_direction(norms[1][0], norms[1][1], norms[1][2])
    d23 = b.add_direction(norms[2][0], norms[2][1], norms[2][2])
    d30 = b.add_direction(norms[3][0], norms[3][1], norms[3][2])
    l01 = b.add_line(p0_id, d01)
    l12 = b.add_line(p1_id, d12)
    l23 = b.add_line(p2_id, d23)
    l30 = b.add_line(p3_id, d30)
    ec01 = b.add_edge_curve(v0, v1, l01)
    ec12 = b.add_edge_curve(v1, v2, l12)
    ec23 = b.add_edge_curve(v2, v3, l23)
    ec30 = b.add_edge_curve(v3, v0, l30)
    oe01 = b.add_oriented_edge(ec01)
    oe12 = b.add_oriented_edge(ec12)
    oe23 = b.add_oriented_edge(ec23)
    oe30 = b.add_oriented_edge(ec30)
    loop = b.add_edge_loop([oe01, oe12, oe23, oe30])
    fob = b.add_face_outer_bound(loop)
    af = b.add_advanced_face([fob], plane)
    return af


def _build_box_faces(b, p000, p100, p110, p010, p001, p101, p111, p011,
                     dx, dy, dz, mx, my, mz):
    face_defs = [
        (p001, p101, p111, p011, p001, dz, dx),
        (p010, p110, p100, p000, p000, mz, dx),
        (p010, p000, p001, p011, p010, mx, dz),
        (p110, p111, p101, p100, p100, dx, dz),
        (p000, p100, p101, p001, p000, my, dx),
        (p011, p111, p110, p010, p011, dy, dx),
    ]
    faces = []
    for pts in face_defs:
        faces.append(_build_box_face(b, *pts))
    return faces


# ── box as full B-rep (ADVANCED_FACE / CLOSED_SHELL chain) ───────────────────


def _step_box(
    sx=1.0, sy=1.0, sz=1.0, cx=0.0, cy=0.0, cz=0.0,
    desc="box", fname="box.step",
):
    b = _STEPBuilder(description=desc, filename=fname)
    hx, hy, hz = sx / 2.0, sy / 2.0, sz / 2.0
    p000 = b.add_point(cx - hx, cy - hy, cz - hz)
    p100 = b.add_point(cx + hx, cy - hy, cz - hz)
    p110 = b.add_point(cx + hx, cy + hy, cz - hz)
    p010 = b.add_point(cx - hx, cy + hy, cz - hz)
    p001 = b.add_point(cx - hx, cy - hy, cz + hz)
    p101 = b.add_point(cx + hx, cy - hy, cz + hz)
    p111 = b.add_point(cx + hx, cy + hy, cz + hz)
    p011 = b.add_point(cx - hx, cy + hy, cz + hz)
    dx = b.add_direction(1, 0, 0)
    dy = b.add_direction(0, 1, 0)
    dz = b.add_direction(0, 0, 1)
    mx = b.add_direction(-1, 0, 0)
    my = b.add_direction(0, -1, 0)
    mz = b.add_direction(0, 0, -1)
    faces = _build_box_faces(b, p000, p100, p110, p010, p001, p101, p111, p011,
                             dx, dy, dz, mx, my, mz)
    shell = b.add_closed_shell(faces)
    solid = b.add_manifold_solid_brep(shell)
    shape = b.add_shape_representation([solid])
    prod = b.add_product(desc, desc, shape)
    _ = prod
    return b.build()


# ── cylinder as CYLINDRICAL_SURFACE + CIRCLE edges ────────────────────────────


def _step_cylinder(radius=1.0, height=2.0, segments=16,
                   desc="cylinder", fname="cylinder.step"):
    b = _STEPBuilder(description=desc, filename=fname)
    hh = height / 2.0
    dz = b.add_direction(0, 0, 1)
    dx = b.add_direction(1, 0, 0)
    mz = b.add_direction(0, 0, -1)
    rim_bot_p = []
    rim_bot_v = []
    rim_top_p = []
    rim_top_v = []
    for i in range(segments):
        angle = 2.0 * math.pi * i / segments
        x = radius * math.cos(angle)
        y = radius * math.sin(angle)
        pb = b.add_point(x, y, -hh)
        pt = b.add_point(x, y, hh)
        rim_bot_p.append(pb)
        rim_bot_v.append(b.add_vertex_point(pb))
        rim_top_p.append(pt)
        rim_top_v.append(b.add_vertex_point(pt))
    c_bot = b.add_point(0, 0, -hh)
    c_top_p = b.add_point(0, 0, hh)
    axis_cyl = b.add_axis2_placement_3d(c_bot, dz, dx)
    cyl_surf = b.add_cylindrical_surface(axis_cyl, radius)
    axis_bot = b.add_axis2_placement_3d(c_bot, mz, dx)
    bot_plane = b.add_plane(axis_bot)
    axis_top = b.add_axis2_placement_3d(c_top_p, dz, dx)
    top_plane = b.add_plane(axis_top)
    shared_bot_edges = []
    for i in range(segments):
        j = (i + 1) % segments
        circ_axis = b.add_axis2_placement_3d(c_bot, dz, dx)
        circ = b.add_circle(circ_axis, radius)
        ec = b.add_edge_curve(rim_bot_v[i], rim_bot_v[j], circ)
        shared_bot_edges.append(b.add_oriented_edge(ec))
    shared_top_edges = []
    for i in range(segments):
        j = (i + 1) % segments
        circ_axis = b.add_axis2_placement_3d(c_top_p, dz, dx)
        circ = b.add_circle(circ_axis, radius)
        ec = b.add_edge_curve(rim_top_v[i], rim_top_v[j], circ)
        shared_top_edges.append(b.add_oriented_edge(ec))
    vert_edges = []
    for i in range(segments):
        line_up = b.add_line(rim_bot_p[i], dz)
        ec = b.add_edge_curve(rim_bot_v[i], rim_top_v[i], line_up)
        vert_edges.append(b.add_oriented_edge(ec))
    vert_rev_edges = []
    for i in range(segments):
        j = (i + 1) % segments
        line_dn = b.add_line(rim_top_p[i], mz)
        ec = b.add_edge_curve(rim_top_v[j], rim_bot_v[j], line_dn, sense=False)
        vert_rev_edges.append(b.add_oriented_edge(ec))
    bot_loop = b.add_edge_loop(shared_bot_edges)
    bot_fob = b.add_face_outer_bound(bot_loop)
    bot_face = b.add_advanced_face([bot_fob], bot_plane)
    top_loop = b.add_edge_loop(shared_top_edges)
    top_fob = b.add_face_outer_bound(top_loop)
    top_face = b.add_advanced_face([top_fob], top_plane)
    side_edges = []
    for i in range(segments):
        side_edges.append(shared_bot_edges[i])
        side_edges.append(vert_edges[i])
        side_edges.append(shared_top_edges[i])
        side_edges.append(vert_rev_edges[i])
    side_loop = b.add_edge_loop(side_edges)
    side_fob = b.add_face_outer_bound(side_loop)
    side_face = b.add_advanced_face([side_fob], cyl_surf)
    shell = b.add_closed_shell([bot_face, top_face, side_face])
    solid = b.add_manifold_solid_brep(shell)
    shape = b.add_shape_representation([solid])
    prod = b.add_product(desc, desc, shape)
    _ = prod
    return b.build()


# ── sphere as SPHERICAL_SURFACE ───────────────────────────────────────────────


def _step_sphere(radius=1.0, desc="sphere", fname="sphere.step"):
    b = _STEPBuilder(description=desc, filename=fname)
    center = b.add_point(0, 0, 0)
    dz = b.add_direction(0, 0, 1)
    dx = b.add_direction(1, 0, 0)
    axis = b.add_axis2_placement_3d(center, dz, dx)
    srf = b.add_spherical_surface(axis, radius)
    equator_pts = []
    n_seg = 16
    for i in range(n_seg):
        angle = 2.0 * math.pi * i / n_seg
        equator_pts.append(b.add_point(radius * math.cos(angle), radius * math.sin(angle), 0))
    top = b.add_point(0, 0, radius)
    bot = b.add_point(0, 0, -radius)
    top_edges = []
    for i in range(n_seg):
        j = (i + 1) % n_seg
        center_axis = b.add_axis2_placement_3d(top, dz, dx)
        circ = b.add_circle(center_axis, 0.001)
        vi = b.add_vertex_point(top)
        vj = b.add_vertex_point(top)
        ec = b.add_edge_curve(vi, vj, circ)
        top_edges.append(b.add_oriented_edge(ec))
    top_loop = b.add_edge_loop(top_edges)
    top_fob = b.add_face_outer_bound(top_loop)
    top_face = b.add_advanced_face([top_fob], srf)
    shell = b.add_closed_shell([top_face])
    solid = b.add_manifold_solid_brep(shell)
    shape = b.add_shape_representation([solid])
    prod = b.add_product(desc, desc, shape)
    _ = prod
    return b.build()


# ── cone as CONICAL_SURFACE ───────────────────────────────────────────────────


def _step_cone(radius=1.0, height=2.0, desc="cone", fname="cone.step"):
    b = _STEPBuilder(description=desc, filename=fname)
    hh = height / 2.0
    c_bot = b.add_point(0, 0, -hh)
    apex = b.add_point(0, 0, hh)
    dz = b.add_direction(0, 0, 1)
    dx = b.add_direction(1, 0, 0)
    mz = b.add_direction(0, 0, -1)
    semi_angle = math.atan2(radius, height)
    n_seg = 16
    rim_bot = []
    for i in range(n_seg):
        angle = 2.0 * math.pi * i / n_seg
        x = radius * math.cos(angle)
        y = radius * math.sin(angle)
        rim_bot.append(b.add_point(x, y, -hh))
    axis_cone = b.add_axis2_placement_3d(c_bot, dz, dx)
    cone_surf = b.add_conical_surface(axis_cone, radius, semi_angle)
    axis_bot = b.add_axis2_placement_3d(c_bot, mz, dx)
    bot_plane = b.add_plane(axis_bot)
    bot_edges = []
    for i in range(n_seg):
        j = (i + 1) % n_seg
        circ_axis = b.add_axis2_placement_3d(c_bot, dz, dx)
        circ = b.add_circle(circ_axis, radius)
        vi = b.add_vertex_point(rim_bot[i])
        vj = b.add_vertex_point(rim_bot[j])
        ec = b.add_edge_curve(vi, vj, circ)
        bot_edges.append(b.add_oriented_edge(ec))
    bot_loop = b.add_edge_loop(bot_edges)
    bot_fob = b.add_face_outer_bound(bot_loop)
    bot_face = b.add_advanced_face([bot_fob], bot_plane)
    side_edges = []
    for i in range(n_seg):
        j = (i + 1) % n_seg
        vi = b.add_vertex_point(rim_bot[i])
        vj = b.add_vertex_point(rim_bot[j])
        va = b.add_vertex_point(apex)
        to_apex_dir = _normalize((0, 0, hh))
        d_up = b.add_direction(to_apex_dir[0], to_apex_dir[1], to_apex_dir[2])
        line_a = b.add_line(rim_bot[i], d_up)
        line_b = b.add_line(rim_bot[j], d_up)
        ec_a = b.add_edge_curve(vi, va, line_a)
        ec_b = b.add_edge_curve(vj, va, line_b)
        side_edges.append(b.add_oriented_edge(ec_a))
        side_edges.append(b.add_oriented_edge(ec_b))
    side_loop = b.add_edge_loop(side_edges)
    side_fob = b.add_face_outer_bound(side_loop)
    side_face = b.add_advanced_face([side_fob], cone_surf)
    shell = b.add_closed_shell([bot_face, side_face])
    solid = b.add_manifold_solid_brep(shell)
    shape = b.add_shape_representation([solid])
    prod = b.add_product(desc, desc, shape)
    _ = prod
    return b.build()


# ── torus as TOROIDAL_SURFACE ─────────────────────────────────────────────────


def _step_torus(major_r=1.0, minor_r=0.4, desc="torus", fname="torus.step"):
    b = _STEPBuilder(description=desc, filename=fname)
    center = b.add_point(0, 0, 0)
    dz = b.add_direction(0, 0, 1)
    dx = b.add_direction(1, 0, 0)
    axis = b.add_axis2_placement_3d(center, dz, dx)
    srf = b.add_toroidal_surface(axis, major_r, minor_r)
    n_rings = 8
    n_seg = 12
    ring_centers = []
    for i in range(n_rings):
        angle = 2.0 * math.pi * i / n_rings
        ring_centers.append(b.add_point(
            major_r * math.cos(angle), major_r * math.sin(angle), 0
        ))
    faces = []
    for ri in range(n_rings):
        rn = (ri + 1) % n_rings
        seg_edges = []
        for si in range(n_seg):
            p0 = b.add_point(0, 0, 0)
            p1 = b.add_point(1, 1, 1)
            v0 = b.add_vertex_point(p0)
            v1 = b.add_vertex_point(p1)
            line_d = b.add_direction(0, 0, 1)
            line = b.add_line(p0, line_d)
            ec = b.add_edge_curve(v0, v1, line)
            seg_edges.append(b.add_oriented_edge(ec))
        loop = b.add_edge_loop(seg_edges)
        fob = b.add_face_outer_bound(loop)
        af = b.add_advanced_face([fob], srf)
        faces.append(af)
    shell = b.add_closed_shell(faces)
    solid = b.add_manifold_solid_brep(shell)
    shape = b.add_shape_representation([solid])
    prod = b.add_product(desc, desc, shape)
    _ = prod
    return b.build()


# ── multi-solid STEP ──────────────────────────────────────────────────────────


def _step_multi_solid(count, desc, fname):
    b = _STEPBuilder(description=desc, filename=fname)
    solids = []
    for i in range(count):
        offset = i * 3.0
        dx = b.add_direction(1, 0, 0)
        dy = b.add_direction(0, 1, 0)
        dz = b.add_direction(0, 0, 1)
        mx = b.add_direction(-1, 0, 0)
        my = b.add_direction(0, -1, 0)
        mz = b.add_direction(0, 0, -1)
        p000 = b.add_point(-0.5 + offset, -0.5, -0.5)
        p100 = b.add_point(0.5 + offset, -0.5, -0.5)
        p110 = b.add_point(0.5 + offset, 0.5, -0.5)
        p010 = b.add_point(-0.5 + offset, 0.5, -0.5)
        p001 = b.add_point(-0.5 + offset, -0.5, 0.5)
        p101 = b.add_point(0.5 + offset, -0.5, 0.5)
        p111 = b.add_point(0.5 + offset, 0.5, 0.5)
        p011 = b.add_point(-0.5 + offset, 0.5, 0.5)
        faces = _build_box_faces(b, p000, p100, p110, p010, p001, p101, p111, p011,
                                 dx, dy, dz, mx, my, mz)
        shell = b.add_closed_shell(faces)
        solid = b.add_manifold_solid_brep(shell)
        solids.append(solid)
    shape = b.add_shape_representation(solids)
    prod = b.add_product(desc, desc, shape)
    _ = prod
    return b.build()


# ── B-spline surface ──────────────────────────────────────────────────────────


def _step_bspline(desc="bspline", fname="bspline.step"):
    b = _STEPBuilder(description=desc, filename=fname)
    cp_grid = [[], [], []]
    for ui in range(3):
        for vi in range(4):
            x = ui * 1.0
            y = vi * 0.8
            z = math.sin(ui * 1.5) * math.cos(vi * 1.2) * 0.5
            cp_grid[ui].append(b.add_point(x, y, z))
    u_degree = 2
    v_degree = 3
    u_multi = [3, 3]
    v_multi = [4, 4]
    u_knots = [0.0, 1.0]
    v_knots = [0.0, 1.0]
    srf = b.add_b_spline_surface_with_knots(
        u_degree, v_degree, cp_grid,
        u_multi, v_multi, u_knots, v_knots,
    )
    corners = [cp_grid[0][0], cp_grid[2][0], cp_grid[2][3], cp_grid[0][3]]
    verts = [b.add_vertex_point(p) for p in corners]
    oedges = []
    for i in range(4):
        d = b.add_direction(1, 0, 0)
        line = b.add_line(corners[i], d)
        e = b.add_edge_curve(verts[i], verts[(i + 1) % 4], line)
        oedges.append(b.add_oriented_edge(e))
    loop = b.add_edge_loop(oedges)
    bound = b.add_face_outer_bound(loop)
    face = b.add_advanced_face([bound], srf)
    shape = b.add_shape_representation([srf, face])
    prod = b.add_product(desc, desc, shape)
    _ = prod
    return b.build()


# ── assembly STEP ─────────────────────────────────────────────────────────────


def _step_assembly(desc="assembly", fname="assembly.step"):
    b = _STEPBuilder(description=desc, filename=fname)
    dx_p1 = b.add_direction(1, 0, 0)
    dy_p1 = b.add_direction(0, 1, 0)
    dz_p1 = b.add_direction(0, 0, 1)
    mx = b.add_direction(-1, 0, 0)
    my = b.add_direction(0, -1, 0)
    mz = b.add_direction(0, 0, -1)
    origin = b.add_point(0, 0, 0)
    axis_p1 = b.add_axis2_placement_3d(origin, dz_p1, dx_p1)
    p000 = b.add_point(-0.5, -0.5, -0.5)
    p100 = b.add_point(0.5, -0.5, -0.5)
    p110 = b.add_point(0.5, 0.5, -0.5)
    p010 = b.add_point(-0.5, 0.5, -0.5)
    p001 = b.add_point(-0.5, -0.5, 0.5)
    p101 = b.add_point(0.5, -0.5, 0.5)
    p111 = b.add_point(0.5, 0.5, 0.5)
    p011 = b.add_point(-0.5, 0.5, 0.5)
    faces = _build_box_faces(b, p000, p100, p110, p010, p001, p101, p111, p011,
                             dx_p1, dy_p1, dz_p1, mx, my, mz)
    shell = b.add_closed_shell(faces)
    solid = b.add_manifold_solid_brep(shell)
    shape = b.add_shape_representation([solid])
    prod_root = b.add_product("assembly_root", desc, shape)
    pdf_root = b.add_product_definition_formation(prod_root, "root_formation")
    pd_root = b.add_product_definition(pdf_root, "root_def", [solid])
    prod_child = b.add_product("part_1", "child part", shape)
    pdf_child = b.add_product_definition_formation(prod_child, "child_formation")
    pd_child = b.add_product_definition(pdf_child, "child_def", [solid])
    trans_origin = b.add_point(2, 0, 0)
    trans_axis = b.add_axis2_placement_3d(trans_origin, dz_p1, dx_p1)
    itd = b.add_item_defined_transformation("place_child", axis_p1, trans_axis)
    b.add_next_assembly_usage_occurrence(pd_child, pd_root, trans_axis)
    _ = itd
    return b.build()


# ── whitespace / comment STEP ─────────────────────────────────────────────────


def _step_whitespace():
    return """ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('unusual whitespace and comments'),'2;1');
FILE_NAME('ws.step','2025-01-01T00:00:00',(''),(''),'exl','exl','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;

  #1  =  CARTESIAN_POINT  (  ''  ,  (  0.0  ,  0.0  ,  0.0  )  )  ;
/* this is a comment */
#2=CARTESIAN_POINT('',(1.0,0.0,0.0));
/*
  multi-line
  comment block
*/
#3=CARTESIAN_POINT('',(1.0,1.0,0.0));
#4=CARTESIAN_POINT('',(0.0,1.0,0.0));
#10=DIRECTION('',(0.0,0.0,1.0));
#11=DIRECTION('',(1.0,0.0,0.0));
#20=AXIS2_PLACEMENT_3D('',#1,#10,#11);
#21=PLANE('',#20);
#30=VERTEX_POINT('',#1); #31=VERTEX_POINT('',#2);
#32=VERTEX_POINT('',#3); #33=VERTEX_POINT('',#4);
#40=LINE('',#1,#11);
#41=EDGE_CURVE('',#30,#31,#40,.T.);
#42=ORIENTED_EDGE('',*,*,#41,.T.);
#50=EDGE_LOOP('',(#42));
#51=FACE_OUTER_BOUND('',#50,.T.);
#52=ADVANCED_FACE('',(#51),#21,.T.);
#100=PRODUCT('ws','ws','',(#52));

ENDSEC;
END-ISO-10303-21;
"""


# ── long filename / non-ASCII STEP ────────────────────────────────────────────


def _step_long_filename():
    return """ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('file with long description and non-ASCII characters \u00e9\u00f1\u00fc'),'2;1');
FILE_NAME('very-long-filename-that-exercises-the-STEP-parser-buffer-handling-logic-edge-case-test-0123456789.step','2025-01-01T00:00:00',('author\u00e9'),('org\u00f1'),'exl corpus generator \u00fcber','exl','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=CARTESIAN_POINT('',(1.,1.,0.));
#4=CARTESIAN_POINT('',(0.,1.,0.));
#10=DIRECTION('',(0.,0.,1.));
#11=DIRECTION('',(1.,0.,0.));
#20=AXIS2_PLACEMENT_3D('',#1,#10,#11);
#21=PLANE('',#20);
#30=VERTEX_POINT('',#1);
#31=VERTEX_POINT('',#2);
#32=VERTEX_POINT('',#3);
#33=VERTEX_POINT('',#4);
#40=LINE('',#1,#11);
#41=EDGE_CURVE('',#30,#31,#40,.T.);
#42=ORIENTED_EDGE('',*,*,#41,.T.);
#50=EDGE_LOOP('',(#42));
#51=FACE_OUTER_BOUND('',#50,.T.);
#52=ADVANCED_FACE('',(#51),#21,.T.);
#100=PRODUCT('long','long','',(#52));
ENDSEC;
END-ISO-10303-21;
"""


# ── malformed STEP (truncated) ────────────────────────────────────────────────


def _step_malformed():
    return """ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('deliberately truncated DATA section'),'2;1');
FILE_NAME('zz-malformed.step','2025-01-01',(''),(''),'exl','exl','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));

"""


# ── small and rotated box variants ────────────────────────────────────────────


def _step_small_box(desc="small box", fname="small-box.step"):
    return _step_box(sx=0.01, sy=0.01, sz=0.01, desc=desc, fname=fname)


def _step_rotated_box(desc="rotated box", fname="box-rotated.step"):
    b = _STEPBuilder(description=desc, filename=fname)
    hx, hy, hz = 0.5, 0.5, 0.5
    cx, cy, cz = 0.0, 0.0, 0.0
    p000 = b.add_point(cx - hx, cy - hy, cz - hz)
    p100 = b.add_point(cx + hx, cy - hy, cz - hz)
    p110 = b.add_point(cx + hx, cy + hy, cz - hz)
    p010 = b.add_point(cx - hx, cy + hy, cz - hz)
    p001 = b.add_point(cx - hx, cy - hy, cz + hz)
    p101 = b.add_point(cx + hx, cy - hy, cz + hz)
    p111 = b.add_point(cx + hx, cy + hy, cz + hz)
    p011 = b.add_point(cx - hx, cy + hy, cz + hz)
    dx = b.add_direction(1, 0, 0)
    dy = b.add_direction(0, 1, 0)
    dz = b.add_direction(0, 0, 1)
    mx = b.add_direction(-1, 0, 0)
    my = b.add_direction(0, -1, 0)
    mz = b.add_direction(0, 0, -1)
    d45 = b.add_direction(0.7071067811865475, 0.0, 0.7071067811865475)
    standard_faces = [
        (p001, p101, p111, p011, p001, dz, dx),
        (p010, p110, p100, p000, p000, mz, dx),
        (p010, p000, p001, p011, p010, mx, dz),
        (p110, p111, p101, p100, p100, dx, dz),
        (p000, p100, p101, p001, p000, my, dx),
    ]
    faces_list = []
    for pts in standard_faces:
        faces_list.append(_build_box_face(b, *pts))
    faces_list.append(_build_box_face(b, p011, p111, p110, p010, p011, dy, d45))
    shell = b.add_closed_shell(faces_list)
    solid = b.add_manifold_solid_brep(shell)
    shape = b.add_shape_representation([solid])
    prod = b.add_product(desc, desc, shape)
    _ = prod
    return b.build()


# ── thin bracket STEP ─────────────────────────────────────────────────────────


def _step_thin_bracket(desc="thin bracket", fname="thin-bracket.step"):
    return _step_box(sx=4.0, sy=0.1, sz=2.0, desc=desc, fname=fname)


# ── main generation ───────────────────────────────────────────────────────────


def _gen_stl():
    box = box_geometry()
    _write(_path("01-box-ascii.stl"), _stl_ascii("box_ascii", box, "box_ascii"))
    _write(_path("02-box-binary.stl"), _stl_binary(box))

    ico_tris, _ = icosphere_geometry(2)
    _write(_path("03-icosphere-sub2-ascii.stl"), _stl_ascii("icosphere_sub2_ascii", ico_tris, "icosphere"))
    _write(_path("04-icosphere-sub2-binary.stl"), _stl_binary(ico_tris))

    cyl16 = cylinder_geometry(radius=1.0, height=2.0, segments=16)
    _write(_path("05-cylinder-16-ascii.stl"), _stl_ascii("cylinder_16", cyl16, "cylinder_16"))

    cyl64 = cylinder_geometry(radius=1.0, height=2.0, segments=64)
    _write(_path("06-cylinder-64-ascii.stl"), _stl_ascii("cylinder_64", cyl64, "cylinder_64"))

    cone = cone_geometry(radius=1.0, height=2.0, segments=24)
    _write(_path("07-cone-ascii.stl"), _stl_ascii("cone", cone, "cone"))

    torus = torus_geometry(major_r=1.0, minor_r=0.4, u_seg=24, v_seg=12)
    _write(_path("08-torus-ascii.stl"), _stl_ascii("torus", torus, "torus"))

    lbracket = l_bracket_geometry()
    _write(_path("09-lbracket-ascii.stl"), _stl_ascii("lbracket", lbracket, "lbracket"))

    plate = plate_geometry()
    _write(_path("10-plate-ascii.stl"), _stl_ascii("plate", plate, "plate"))

    openbox = open_box_geometry()
    _write(_path("11-open-box-ascii.stl"), _stl_ascii("open_box", openbox, "open_box"))

    degenerate = degenerate_tri_geometry()
    _write(_path("12-degenerate-ascii.stl"), _stl_ascii("degenerate", degenerate, "degenerate"))

    ico_large, _ = icosphere_geometry(5)
    _write(_path("13-icosphere-10k-binary.stl"), _stl_binary(ico_large))

    sphere = sphere_uv_geometry(radius=1.0, u_seg=24, v_seg=12)
    _write(_path("14-sphere-ascii.stl"), _stl_ascii("sphere", sphere, "sphere"))

    plate_thin = plate_geometry(w=3, d=2, t=0.001)
    _write(_path("15-thin-plate-binary.stl"), _stl_binary(plate_thin))

    _write(_path("16-lbracket-binary.stl"), _stl_binary(lbracket))


def _gen_obj():
    box = box_geometry()
    _write(_path("18-box.obj"), _obj_from_tris("box", box))

    ico_tris, _ = icosphere_geometry(2)
    _write(_path("19-icosphere.obj"), _obj_from_tris("icosphere", ico_tris))

    cyl = cylinder_geometry()
    _write(_path("20-cylinder.obj"), _obj_from_tris("cylinder", cyl))

    cone = cone_geometry()
    _write(_path("21-cone.obj"), _obj_from_tris("cone", cone))

    torus = torus_geometry()
    _write(_path("22-torus.obj"), _obj_from_tris("torus", torus))

    box_small_export = box_geometry(1, 1, 1)
    _write(_path("23-quads-only.obj"), _obj_quads_only("quads_only", box_small_export))

    _write(_path("24-mixed-tri-quad.obj"), _obj_mixed("mixed_tri_quad", box_small_export))

    _write(_path("25-negative-index.obj"), _obj_negative_index("negative_index", box_small_export))

    multi_tris, groups = _obj_multi_group_geometry()
    _write(_path("26-multi-group.obj"), _obj_from_tris_with_groups("multi_group", multi_tris, groups))

    _write(_path("27-vn-vt.obj"), _obj_normals_texcoords("vn_vt", box_small_export))

    _write(_path("28-usemtl.obj"), _obj_usemtl("usemtl", box_small_export))

    ico_10k, _ = icosphere_geometry(5)
    _write(_path("29-10k.obj"), _obj_from_tris("ico_10k", ico_10k))

    lbracket = l_bracket_geometry()
    _write(_path("30-lbracket.obj"), _obj_from_tris("lbracket", lbracket))

    plate = plate_geometry()
    _write(_path("31-plate.obj"), _obj_from_tris("plate", plate))

    content = _obj_from_tris("no_newline", box_small_export).rstrip("\n")
    _write(_path("32-no-trailing-newline.obj"), content)


def _gen_step():
    _write(_path("33-box.step"), _step_box(sx=1.0, sy=1.0, sz=1.0, desc="box", fname="box.step"))
    _write(_path("34-box-large.step"), _step_box(sx=10.0, sy=8.0, sz=6.0, desc="large box", fname="box-large.step"))
    _write(_path("35-cylinder.step"), _step_cylinder(radius=1.0, height=2.0, segments=16, desc="cylinder", fname="cylinder.step"))
    _write(_path("36-sphere.step"), _step_sphere(radius=1.0, desc="sphere", fname="sphere.step"))
    _write(_path("37-cone.step"), _step_cone(radius=1.0, height=2.0, desc="cone", fname="cone.step"))
    _write(_path("38-torus.step"), _step_torus(major_r=1.0, minor_r=0.4, desc="torus", fname="torus.step"))
    _write(_path("39-multi-solid-2.step"), _step_multi_solid(2, "multi solid 2", "multi-solid-2.step"))
    _write(_path("40-multi-solid-3.step"), _step_multi_solid(3, "multi solid 3", "multi-solid-3.step"))
    _write(_path("41-multi-solid-5.step"), _step_multi_solid(5, "multi solid 5", "multi-solid-5.step"))
    _write(_path("42-bspline.step"), _step_bspline(desc="bspline surface", fname="bspline.step"))
    _write(_path("43-assembly.step"), _step_assembly(desc="assembly", fname="assembly.step"))
    _write(_path("44-whitespace.step"), _step_whitespace())
    _write(_path("45-long-filename.step"), _step_long_filename())
    _write(_path("46-zz-malformed.step"), _step_malformed())
    _write(_path("47-small-box.step"), _step_small_box(desc="small box", fname="small-box.step"))
    _write(_path("48-box-rotated.step"), _step_rotated_box(desc="rotated box", fname="box-rotated.step"))
    _write(_path("49-thin-bracket.step"), _step_thin_bracket(desc="thin bracket", fname="thin-bracket.step"))


def _gen_misc():
    _write(_path("50-empty.stl"), _stl_ascii("empty", [], "empty"))
    _write(_path("51-verts-only.obj"), "o verts_only\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0.5 0.5 0.5\n")
    _write(_path("52-crlf.stl"), _stl_crlf(box_geometry(), "crlf_cube"))


if __name__ == "__main__":
    _gen_stl()
    _gen_obj()
    _gen_step()
    _gen_misc()
    print("Corpus generation complete.")
