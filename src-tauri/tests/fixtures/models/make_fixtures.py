#!/usr/bin/env python3
"""Regenerate the binary 3D-model fixtures next to this script.

Stdlib only. cube-ascii.stl and cube.obj are hand-written and not touched here.
Output is deterministic (fixed zip timestamps), so a rerun is a no-op diff.
"""

import os
import struct
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
SIZE = 20.0

# A 20 mm cube as 12 outward-facing triangles: (normal, (v1, v2, v3)).
_V = [
    (0, 0, 0), (SIZE, 0, 0), (SIZE, SIZE, 0), (0, SIZE, 0),
    (0, 0, SIZE), (SIZE, 0, SIZE), (SIZE, SIZE, SIZE), (0, SIZE, SIZE),
]
_FACES = [
    ((0, 0, -1), (0, 2, 1)), ((0, 0, -1), (0, 3, 2)),
    ((0, 0, 1), (4, 5, 6)), ((0, 0, 1), (4, 6, 7)),
    ((0, -1, 0), (0, 1, 5)), ((0, -1, 0), (0, 5, 4)),
    ((1, 0, 0), (1, 2, 6)), ((1, 0, 0), (1, 6, 5)),
    ((0, 1, 0), (2, 3, 7)), ((0, 1, 0), (2, 7, 6)),
    ((-1, 0, 0), (3, 0, 4)), ((-1, 0, 0), (3, 4, 7)),
]


def binary_stl(header):
    out = bytearray(header.ljust(80, b"\0")[:80])
    out += struct.pack("<I", len(_FACES))
    for normal, idx in _FACES:
        out += struct.pack("<3f", *normal)
        for i in idx:
            out += struct.pack("<3f", *_V[i])
        out += struct.pack("<H", 0)
    return bytes(out)


def mesh_xml(object_id):
    verts = "".join(
        f'<vertex x="{x:g}" y="{y:g}" z="{z:g}"/>' for x, y, z in _V
    )
    tris = "".join(
        f'<triangle v1="{a}" v2="{b}" v3="{c}"/>' for _, (a, b, c) in _FACES
    )
    return (
        f'<object id="{object_id}" type="model"><mesh>'
        f"<vertices>{verts}</vertices><triangles>{tris}</triangles>"
        "</mesh></object>"
    )


CONTENT_TYPES = (
    '<?xml version="1.0" encoding="UTF-8"?>\n'
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    '<Default Extension="rels" '
    'ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    '<Default Extension="model" '
    'ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>'
    "</Types>"
)

RELS = (
    '<?xml version="1.0" encoding="UTF-8"?>\n'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Target="/3D/3dmodel.model" Id="rel0" '
    'Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>'
    "</Relationships>"
)

MODEL_NS = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02"


def two_parts_model():
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        f'<model unit="millimeter" xml:lang="en-US" xmlns="{MODEL_NS}">'
        f"<resources>{mesh_xml(1)}{mesh_xml(2)}</resources>"
        "<build>"
        '<item objectid="1"/>'
        '<item objectid="2" transform="1 0 0 0 1 0 0 0 1 30 0 0"/>'
        "</build></model>"
    )


def write_zip(name, entries):
    path = os.path.join(HERE, name)
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as zf:
        for entry_name, text in entries:
            info = zipfile.ZipInfo(entry_name, date_time=(2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            zf.writestr(info, text)


def write_bytes(name, data):
    with open(os.path.join(HERE, name), "wb") as f:
        f.write(data)


def main():
    write_bytes("cube-binary.stl", binary_stl(b"mdviewer fixture: 20 mm cube"))
    write_bytes("solid-header-binary.stl", binary_stl(b"solid but actually binary"))
    plain = [
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", RELS),
        ("3D/3dmodel.model", two_parts_model()),
    ]
    write_zip("two-parts.3mf", plain)
    write_zip(
        "bambu-project.3mf",
        plain
        + [
            (
                "3D/Objects/object_1.model",
                '<?xml version="1.0" encoding="UTF-8"?>\n'
                f'<model unit="millimeter" xmlns="{MODEL_NS}">'
                f"<resources>{mesh_xml(1)}</resources><build/></model>",
            )
        ],
    )


if __name__ == "__main__":
    main()
