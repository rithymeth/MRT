# mrt-ai

The first native machine-learning backend for MRT.

This MVP intentionally has zero external dependencies and provides 2D Tensor,
matrix multiplication, trainable Dense layers, ReLU, mean-squared-error loss,
SGD, Sequential models, deterministic initialization, tests, and a linear
regression example.

The backend-first approach keeps tensor math in Rust. The next integration layer
will expose these primitives as native MRT values/functions inside mrt-interp.

Roadmap:
1. Current: Tensor + Dense/ReLU + MSE + SGD.
2. Adam/AdamW and minibatches.
3. Autograd graph/tape.
4. Softmax/cross-entropy and classification metrics.
5. Serialization/checkpoints.
6. Bind the API into the MRT interpreter.
7. Bytecode/VM-friendly tensor representation.
8. Optional SIMD/GPU backends.
