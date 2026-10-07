# Materials — the `m3` material extras

glTF materials cover diffuse, normal, emissive and occlusion textures and three
alpha modes. Two things M3 materials do that glTF cannot say are written into
the material's `extras` under `m3`; the object is absent when neither applies.

```json
{"name": "material_3", "alphaMode": "BLEND",
 "extras": {"m3": {"blend": "add", "uv_scroll": [0, -0.5]}}}
```

| Field | Meaning |
|---|---|
| `blend` | `add` · `alpha_add` · `multiply`. The material is also `alphaMode: BLEND`, so a plain viewer draws it alpha-blended; an engine that reads this field should switch to the named blend. Absent for opaque and ordinary alpha-blended materials. `MAT_` and `MADD` materials both carry it. |
| `uv_scroll` | UV units per second to add to the texture coordinates. `MAT_` only. |

## Where the numbers come from

* `blend` is the material's blend mode: 2 → `add`, 3 → `alpha_add`, 4 and 5 →
  `multiply`.
* `uv_scroll` is the slope of a layer's animated `uv_offset` between its first
  and last key, taken from the idle (`Stand…`) sequence, or the first sequence
  that animates it when there is no idle one. glTF has one texture transform
  per material, so the first layer that moves speaks for the whole material,
  in the order diffuse, emissive, alpha masks, specular, second emissive — a
  flowing river is often a still colour under a drifting alpha mask. A track
  that loops back and forth reports its net drift, which may be zero.

## At rest

A layer's `color_multiply × color_brightness` is applied to the factors glTF
does have: an emissive layer scales `emissiveFactor`, an additive layer scales
`baseColorFactor`, an alpha-blended one scales its alpha. Glows that only an
animation switches on are authored at zero and so export dark, matching the
model's idle look.
