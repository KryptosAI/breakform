# corpus

Hand-authored test fixtures and procedurally generated benchmark models for EXL
interoperability testing.  Generate the full corpus with:

```
python3 scripts/gen_corpus.py
```

The three files listed first are hand-authored and must be preserved byte-for-byte.
All others are auto-generated.

## Table of all corpus files

| file | format | size | features exercised | expected |
|------|--------|------|--------------------|----------|
| `cube-ascii.stl` *(hand)* | STL ASCII | tiny | unit cube, 12 tris, named solid | ok |
| `quad.obj` *(hand)* | OBJ | tiny | single quad face in group `base_plate` | ok |
| `bracket.step` *(hand)* | STEP | tiny | minimal ISO-10303-21: 4 points, editable line, product | ok |
| `01-box-ascii.stl` | STL ASCII | tiny | box mesh, ASCII format, `solid`/`endsolid` grammar | ok |
| `02-box-binary.stl` | STL binary | tiny | box mesh, binary 80+4+50N format, little-endian | ok |
| `03-icosphere-sub2-ascii.stl` | STL ASCII | small | subdivided icosahedron (sub=2, 320 faces), ASCII | ok |
| `04-icosphere-sub2-binary.stl` | STL binary | small | subdivided icosahedron (sub=2, 320 faces), binary | ok |
| `05-cylinder-16-ascii.stl` | STL ASCII | small | triangulated cylinder (16 segments), ASCII | ok |
| `06-cylinder-64-ascii.stl` | STL ASCII | medium | triangulated cylinder (64 segments, 384 faces), ASCII | ok |
| `07-cone-ascii.stl` | STL ASCII | small | triangulated cone (24 segments), ASCII | ok |
| `08-torus-ascii.stl` | STL ASCII | medium | parametric torus (24x12 grid, 576 faces), ASCII | ok |
| `09-lbracket-ascii.stl` | STL ASCII | small | extruded L-bracket shape, non-convex, ASCII | ok |
| `10-plate-ascii.stl` | STL ASCII | tiny | thin plate (3x2x0.1), ASCII | ok |
| `11-open-box-ascii.stl` | STL ASCII | small | box missing top face, non-watertight, ASCII | ok |
| `12-degenerate-ascii.stl` | STL ASCII | tiny | mix of collinear and degenerate triangles, ASCII | ok |
| `13-icosphere-10k-binary.stl` | STL binary | large | icosahedron sub=5 (20480 faces), binary | ok |
| `14-sphere-ascii.stl` | STL ASCII | medium | UV-sphere (24x12 grid), ASCII | ok |
| `15-thin-plate-binary.stl` | STL binary | tiny | extremely thin plate (t=0.001), binary | ok |
| `16-lbracket-binary.stl` | STL binary | small | L-bracket in binary format | ok |
| `18-box.obj` | OBJ | tiny | box mesh, standard OBJ with `o`/`v`/`f` | ok |
| `19-icosphere.obj` | OBJ | small | icosahedron sub=2, 320 faces | ok |
| `20-cylinder.obj` | OBJ | small | triangulated cylinder (16 segments) | ok |
| `21-cone.obj` | OBJ | small | triangulated cone (24 segments) | ok |
| `22-torus.obj` | OBJ | medium | parametric torus (24x12) | ok |
| `23-quads-only.obj` | OBJ | tiny | all faces are quads (`f a b c d`), no triangles | ok |
| `24-mixed-tri-quad.obj` | OBJ | small | mix of triangle and quad faces in single file | ok |
| `25-negative-index.obj` | OBJ | tiny | faces use relative (negative) vertex indices | ok |
| `26-multi-group.obj` | OBJ | small | three `g` groups: `box_left`, `box_center`, `box_right` | ok |
| `27-vn-vt.obj` | OBJ | tiny | includes `vn` normals and `vt` texture coordinates | ok |
| `28-usemtl.obj` | OBJ | tiny | `mtllib test.mtl`, `usemtl default_material` (unresolved) | ok |
| `29-10k.obj` | OBJ | large | icosahedron sub=5, 20480 faces | ok |
| `30-lbracket.obj` | OBJ | small | L-bracket as OBJ | ok |
| `31-plate.obj` | OBJ | tiny | thin plate as OBJ | ok |
| `32-no-trailing-newline.obj` | OBJ | tiny | valid OBJ with no trailing `\n`, tests EOF handling | ok |
| `33-box.step` | STEP | small | full B-rep: CARTESIAN_POINT, DIRECTION, AXIS2_PLACEMENT_3D, PLANE, VERTEX_POINT, LINE, EDGE_CURVE, ORIENTED_EDGE, EDGE_LOOP, FACE_OUTER_BOUND, ADVANCED_FACE, CLOSED_SHELL, MANIFOLD_SOLID_BREP, PRODUCT | ok |
| `34-box-large.step` | STEP | small | box with large dimensions (10x8x6) | ok |
| `35-cylinder.step` | STEP | medium | CYLINDRICAL_SURFACE + CIRCLE edges, 16 segments | ok |
| `36-sphere.step` | STEP | small | SPHERICAL_SURFACE entity | ok |
| `37-cone.step` | STEP | medium | CONICAL_SURFACE entity with semi_angle | ok |
| `38-torus.step` | STEP | large | TOROIDAL_SURFACE entity | ok |
| `39-multi-solid-2.step` | STEP | medium | 2 MANIFOLD_SOLID_BREP / CLOSED_SHELL instances | ok |
| `40-multi-solid-3.step` | STEP | large | 3 MANIFOLD_SOLID_BREP instances | ok |
| `41-multi-solid-5.step` | STEP | large | 5 MANIFOLD_SOLID_BREP instances | ok |
| `42-bspline.step` | STEP | small | B_SPLINE_SURFACE_WITH_KNOTS (params expected dropped by importer) | ok/degraded |
| `43-assembly.step` | STEP | medium | PRODUCT, PRODUCT_DEFINITION_FORMATION, PRODUCT_DEFINITION, NEXT_ASSEMBLY_USAGE_OCCURRENCE, ITEM_DEFINED_TRANSFORMATION, AXIS2_PLACEMENT_3D | ok |
| `44-whitespace.step` | STEP | small | extra whitespace, inline `/* comments */`, multi-line entities | ok |
| `45-long-filename.step` | STEP | small | long FILE_NAME, non-ASCII characters (`\u00e9\u00f1\u00fc`) in strings | ok |
| `46-zz-malformed.step` | STEP | tiny | truncated DATA section, no ENDSEC, no END-ISO | **fail** |
| `47-small-box.step` | STEP | tiny | box with very small dimensions (0.01x0.01x0.01) | ok |
| `48-box-rotated.step` | STEP | small | box with one face having a 45-deg rotated reference direction | ok |
| `49-thin-bracket.step` | STEP | small | thin plate box (4x0.1x2) | ok |
| `50-empty.stl` | STL ASCII | tiny | ASCII STL with zero facets (`solid empty`/`endsolid empty`) | ok |
| `51-verts-only.obj` | OBJ | tiny | OBJ with `v` records but no `f` faces | ok |
| `52-crlf.stl` | STL ASCII | small | box mesh with CRLF (`\r\n`) line endings | ok |
