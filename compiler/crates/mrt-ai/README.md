# MRT-AI

The first native machine-learning backend for MRT.

MRT-AI currently provides a small, dependency-free training engine implemented in Rust:

- 2D tensors
- matrix multiplication
- dense layers
- ReLU
- mean-squared error
- SGD
- sequential models
- deterministic initialization
- full-batch training

## Run directly from MRT

AI training is exposed to the Rust MRT interpreter as a native built-in, so an `.mrt` program can train a model without embedding Rust code.

Example:

    var result = aiTrainLinear([[0], [1], [2], [3]], [[1], [3], [5], [7]], 500, 0.05);
    print(result.loss);
    print(result.predictions);

Run it with the MRT CLI:

    cargo run --manifest-path compiler/Cargo.toml -p mrt-cli -- examples/ai_linear.mrt

The built-in API is `aiTrainLinear(x, y, epochs, learningRate)`. It accepts 2D MRT arrays and returns `initialLoss`, `loss`, `predictions`, and `epochs`.

## Roadmap

1. Tensor + Dense/ReLU + MSE + SGD
2. Native MRT training API
3. Adam/AdamW and minibatches
4. Autograd graph/tape
5. Softmax, cross-entropy, and classification metrics
6. Serialization and checkpoints
7. Bytecode/VM-friendly tensor representation
8. Optional SIMD/GPU backends
9. Higher-level `mrt/ai` standard library API