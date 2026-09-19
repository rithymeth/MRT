# MRT-AI

The machine-learning engine extension for MRT.

It is an **engine extension**, not part of the language: only the Rust engines
have it, a program using it is not portable, and it is deliberately outside
the conformance corpus. See "Engine extensions" in `docs/LANGUAGE_SPEC.md` for
what that means and why the distinction is written down.

## What is here

Two layers, and the second is the one that matters.

`lib.rs` holds the original hand-differentiated engine: `Tensor`, `Dense`,
`Relu`, `Sequential`, `Sgd`, `Trainer`. Every layer in it carries its own
backward pass, written by hand and checked by eye.

`autograd.rs` replaced that arrangement. A `Tape` records each operation as it
runs and one reverse scan produces every gradient, so a new layer contributes
a *forward* rule and nothing else. The ops are add, sub, mul, matmul, scale,
add_bias, relu, sigmoid, tanh, exp, transpose, softmax, sum, mean, reshape,
im2col, planar, row_mean, broadcast, rsqrt and mul_row. Mean squared error is
not an op at all: it is a composition of three of them.

`nn.rs` is what that bought. `Dense`, `Conv2d`, `LayerNorm`, `Attention`,
`Block` (a transformer block) and the `Adam` optimiser. `Conv2d`'s forward is
four lines over `im2col` and contributes no derivative; `Attention`
contributes none at all; `LayerNorm` is composed rather than derived.

`check_gradient` compares a backward rule against central finite differences,
and is the only evidence any of it is right. It is not sufficient evidence:
an initialiser bug once left every gradient correct *and zero*, which no
gradient check can see. Tests that train to a real criterion catch that class.

## From MRT

A model is an array of layer objects — ordinary MRT data, not an opaque
handle — so a program can print it, index it and read a weight:

```mrt
func main() {
    var model = [
        aiDense(2, 4, 7),
        aiActivation("tanh"),
        aiDense(4, 1, 8),
        aiActivation("sigmoid")
    ];

    // XOR: no single dense layer can learn it.
    var trained = aiTrain(model, [[0, 0], [0, 1], [1, 0], [1, 1]],
                                 [[0], [1], [1], [0]], 400, 0.1);
    print(trained.initialLoss, "->", trained.loss);
    print(aiPredict(trained.model, [[1, 0]]));
}
```

Run it with:

    cargo run --manifest-path compiler/Cargo.toml --bin mrt-run -- examples/ai_net.mrt

The built-ins are `aiDense`, `aiConv2d`, `aiAttention`, `aiLayerNorm`,
`aiActivation`, `aiPredict` and `aiTrain`, plus `aiTrainLinear(x, y, epochs,
rate)` — the one-call shortcut for a single dense layer that predates the
model surface. `docs/LANGUAGE_SPEC.md` has the signatures. The bridge itself
is `compiler/crates/mrt-interp/src/ai.rs`.

## Roadmap

Done: tensors, a native MRT training API, Adam, an autograd tape, softmax,
convolutions, attention and layer norm, and a model surface MRT can inspect.

Left: cross-entropy and classification metrics, minibatches, serialisation and
checkpoints, and optional SIMD/GPU backends. The last of those is a different
kind of project from everything above it and is not queued behind the rest.
