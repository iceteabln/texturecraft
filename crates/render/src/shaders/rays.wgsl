// Shared ray casting of the GPU path tracer (trace.wgsl) and the GPU baker (bake.wgsl), each
// concatenated after this file: the BVH buffers (gpu_trace.rs `bvh_rows` layout), Möller–Trumbore
// with the CPU's edge slack, and the traversal of `texturecraft_geom::Bvh`. The including shader
// defines `fn accept(i: u32, d: vec3<f32>) -> bool`, which filters the triangles a ray may hit
// (BVH order index, ray direction).

const NONE: u32 = 0xffffffffu;
const EDGE: f32 = 1e-5;

// Per node: (lo, first child or triangle), (hi, triangle count; 0 for inner nodes), (parent, _, _, _);
// floats as bits. The parent link keeps the walk in registers: a per-thread stack of 64 lived in
// local memory and dominated bake time at 4K with subsampling.
@group(0) @binding(1) var<storage, read> nodes: array<vec4<u32>>;
// Per triangle in BVH order: (p0, w), (p1, w), (p2, w), (u32 x4); floats as bits, the w words and
// last row are the including shader's.
@group(0) @binding(2) var<storage, read> tris: array<vec4<u32>>;

fn max3(c: vec3<f32>) -> f32 {
    return max(c.x, max(c.y, c.z));
}

// Duff et al. 2017 orthonormal basis (glam's `any_orthonormal_pair`).
fn onb_t(n: vec3<f32>) -> vec3<f32> {
    let s = select(-1.0, 1.0, n.z >= 0.0);
    let a = -1.0 / (s + n.z);
    let b = n.x * n.y * a;
    return vec3<f32>(1.0 + s * n.x * n.x * a, s * b, -s * n.x);
}

fn onb_b(n: vec3<f32>) -> vec3<f32> {
    let s = select(-1.0, 1.0, n.z >= 0.0);
    let a = -1.0 / (s + n.z);
    let b = n.x * n.y * a;
    return vec3<f32>(b, s + n.y * n.y * a, -n.y);
}

struct Hit {
    t: f32,
    idx: u32,
    u: f32,
    v: f32,
};

fn slab(n: u32, o: vec3<f32>, inv: vec3<f32>, tmax: f32) -> f32 {
    let lo = bitcast<vec3<f32>>(nodes[n * 3u].xyz);
    let hi = bitcast<vec3<f32>>(nodes[n * 3u + 1u].xyz);
    let t0 = (lo - o) * inv;
    let t1 = (hi - o) * inv;
    let tn = max(max3(min(t0, t1)), 0.0);
    let tf = min(min(min(max(t0.x, t1.x), max(t0.y, t1.y)), max(t0.z, t1.z)), tmax);
    return select(-1.0, tn, tn <= tf);
}

// Vertex `k` of triangle `i` (BVH order).
fn tri_p(i: u32, k: u32) -> vec3<f32> {
    return bitcast<vec3<f32>>(tris[i * 4u + k].xyz);
}

// Möller–Trumbore, double sided, with the CPU's edge slack: (t, u, v), t < 0 for a miss.
fn hit_tri(i: u32, o: vec3<f32>, d: vec3<f32>) -> vec3<f32> {
    let miss = vec3<f32>(-1.0);
    let p0 = tri_p(i, 0u);
    let e1 = tri_p(i, 1u) - p0;
    let e2 = tri_p(i, 2u) - p0;
    let p = cross(d, e2);
    let det = dot(e1, p);
    if (abs(det) < 1e-12) {
        return miss;
    }
    let inv = 1.0 / det;
    let s = o - p0;
    let u = dot(s, p) * inv;
    if (u < -EDGE || u > 1.0 + EDGE) {
        return miss;
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if (v < -EDGE || u + v > 1.0 + EDGE) {
        return miss;
    }
    let t = dot(e2, q) * inv;
    return select(miss, vec3<f32>(t, u, v), t > 0.0);
}

// The child entered first. Distances are against the ray's original limit, so the choice does not
// change when a closer hit shrinks the search.
fn nearer_child(left: u32, right: u32, dl: f32, dr: f32) -> u32 {
    if (dl >= 0.0 && (dr < 0.0 || dl <= dr)) {
        return left;
    }
    if (dr >= 0.0) {
        return right;
    }
    return NONE;
}

// Closest accepted hit before `tmax` (or any, for shadow rays) in the BVH rooted at node `root`;
// idx NONE when nothing is hit. Parent links replace the traversal stack.
fn traverse_from(root: u32, o: vec3<f32>, d: vec3<f32>, tmax: f32, any_hit: bool) -> Hit {
    var best = Hit(tmax, NONE, 0.0, 0.0);
    // Zero direction components would give 0·∞ in the slab test.
    let sd = select(d, select(vec3<f32>(-1e-20), vec3<f32>(1e-20), d >= vec3<f32>(0.0)), abs(d) < vec3<f32>(1e-20));
    let inv = 1.0 / sd;
    if (slab(root, o, inv, best.t) < 0.0) {
        return best;
    }
    var node = root;
    var prev = NONE;
    // A correct walk steps at most three times per node (enter, return from the near child,
    // return from the far one). The cap turns a malformed tree into a miss instead of a hang.
    var steps = 0u;
    loop {
        steps += 1u;
        if (node == NONE || steps > 16777216u) {
            break;
        }
        let parent = nodes[node * 3u + 2u].x;
        let first = nodes[node * 3u].w;
        let count = nodes[node * 3u + 1u].w;
        if (prev == parent) {
            if (count > 0u) {
                for (var i = first; i < first + count; i++) {
                    let h = hit_tri(i, o, d);
                    if (h.x > 0.0 && h.x < best.t && accept(i, d)) {
                        best = Hit(h.x, i, h.y, h.z);
                        if (any_hit) {
                            return best;
                        }
                    }
                }
                prev = node;
                node = parent;
            } else {
                let left = first;
                let right = first + 1u;
                let child = nearer_child(left, right, slab(left, o, inv, best.t), slab(right, o, inv, best.t));
                prev = node;
                node = select(parent, child, child != NONE);
            }
        } else if (count > 0u) {
            prev = node;
            node = parent;
        } else {
            let left = first;
            let right = first + 1u;
            let near = nearer_child(left, right, slab(left, o, inv, tmax), slab(right, o, inv, tmax));
            let far = select(left, right, near == left);
            if (near != NONE && prev == near && slab(far, o, inv, best.t) >= 0.0) {
                prev = node;
                node = far;
            } else {
                prev = node;
                node = parent;
            }
        }
    }
    return best;
}
