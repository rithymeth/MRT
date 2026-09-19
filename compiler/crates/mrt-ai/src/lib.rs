pub mod autograd;
pub mod nn;

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub struct Tensor {
    pub data: Vec<f64>,
    pub rows: usize,
    pub cols: usize,
}

impl Tensor {
    pub fn zeros(rows: usize, cols: usize) -> Self {
        Self {
            data: vec![0.0; rows * cols],
            rows,
            cols,
        }
    }

    pub fn from_vec(rows: usize, cols: usize, data: Vec<f64>) -> Result<Self, AiError> {
        if rows * cols != data.len() {
            return Err(AiError::Shape(format!(
                "expected {} values for shape ({rows}, {cols}), got {}",
                rows * cols,
                data.len()
            )));
        }
        Ok(Self { data, rows, cols })
    }

    pub fn get(&self, row: usize, col: usize) -> f64 {
        self.data[row * self.cols + col]
    }
    pub fn set(&mut self, row: usize, col: usize, value: f64) {
        self.data[row * self.cols + col] = value;
    }

    pub fn map(&self, f: impl Fn(f64) -> f64) -> Self {
        Self {
            data: self.data.iter().copied().map(f).collect(),
            rows: self.rows,
            cols: self.cols,
        }
    }

    pub fn add(&self, other: &Self) -> Result<Self, AiError> {
        self.binary(other, |a, b| a + b)
    }

    pub fn sub(&self, other: &Self) -> Result<Self, AiError> {
        self.binary(other, |a, b| a - b)
    }

    fn binary(&self, other: &Self, f: impl Fn(f64, f64) -> f64) -> Result<Self, AiError> {
        if self.rows != other.rows || self.cols != other.cols {
            return Err(AiError::Shape("tensor shapes must match".into()));
        }
        Ok(Self {
            data: self
                .data
                .iter()
                .zip(&other.data)
                .map(|(a, b)| f(*a, *b))
                .collect(),
            rows: self.rows,
            cols: self.cols,
        })
    }

    pub fn matmul(&self, other: &Self) -> Result<Self, AiError> {
        if self.cols != other.rows {
            return Err(AiError::Shape(format!(
                "cannot multiply ({}, {}) by ({}, {})",
                self.rows, self.cols, other.rows, other.cols
            )));
        }
        let mut out = Self::zeros(self.rows, other.cols);
        for i in 0..self.rows {
            for k in 0..self.cols {
                let a = self.get(i, k);
                for j in 0..other.cols {
                    out.data[i * other.cols + j] += a * other.get(k, j);
                }
            }
        }
        Ok(out)
    }

    pub fn transpose(&self) -> Self {
        let mut out = Self::zeros(self.cols, self.rows);
        for r in 0..self.rows {
            for c in 0..self.cols {
                out.set(c, r, self.get(r, c));
            }
        }
        out
    }

    pub fn scale(&self, value: f64) -> Self {
        self.map(|x| x * value)
    }
    fn mul(&self, other: &Self) -> Result<Self, AiError> {
        self.binary(other, |a, b| a * b)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AiError {
    Shape(String),
    Training(String),
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shape(s) => write!(f, "shape error: {s}"),
            Self::Training(s) => write!(f, "training error: {s}"),
        }
    }
}
impl std::error::Error for AiError {}

pub trait Layer {
    fn forward(&mut self, input: &Tensor) -> Result<Tensor, AiError>;
    fn backward(&mut self, grad: &Tensor) -> Result<Tensor, AiError>;
    fn update(&mut self, optimizer: &mut Sgd);
}

pub struct Dense {
    pub weights: Tensor,
    pub bias: Tensor,
    input: Option<Tensor>,
    grad_w: Tensor,
    grad_b: Tensor,
}

impl Dense {
    pub fn new(input: usize, output: usize, seed: u64) -> Self {
        let mut rng = XorShift64::new(seed);
        let scale = (2.0 / input as f64).sqrt();
        let weights = Tensor {
            data: (0..input * output)
                .map(|_| (rng.next_f64() * 2.0 - 1.0) * scale)
                .collect(),
            rows: input,
            cols: output,
        };
        Self {
            weights,
            bias: Tensor::zeros(1, output),
            input: None,
            grad_w: Tensor::zeros(input, output),
            grad_b: Tensor::zeros(1, output),
        }
    }
}

impl Layer for Dense {
    fn forward(&mut self, input: &Tensor) -> Result<Tensor, AiError> {
        if input.cols != self.weights.rows {
            return Err(AiError::Shape(format!(
                "Dense expected {} features, got {}",
                self.weights.rows, input.cols
            )));
        }
        self.input = Some(input.clone());
        let mut out = input.matmul(&self.weights)?;
        for r in 0..out.rows {
            for c in 0..out.cols {
                out.set(r, c, out.get(r, c) + self.bias.get(0, c));
            }
        }
        Ok(out)
    }

    fn backward(&mut self, grad: &Tensor) -> Result<Tensor, AiError> {
        let input = self
            .input
            .as_ref()
            .ok_or_else(|| AiError::Training("backward called before forward".into()))?;
        self.grad_w = input.transpose().matmul(grad)?;
        self.grad_b = Tensor::zeros(1, grad.cols);
        for c in 0..grad.cols {
            self.grad_b
                .set(0, c, (0..grad.rows).map(|r| grad.get(r, c)).sum());
        }
        grad.matmul(&self.weights.transpose())
    }

    fn update(&mut self, optimizer: &mut Sgd) {
        optimizer.update(&mut self.weights, &self.grad_w);
        optimizer.update(&mut self.bias, &self.grad_b);
    }
}

pub struct Relu {
    mask: Option<Tensor>,
}
impl Relu {
    pub fn new() -> Self {
        Self { mask: None }
    }
}
impl Default for Relu {
    fn default() -> Self {
        Self::new()
    }
}

impl Layer for Relu {
    fn forward(&mut self, input: &Tensor) -> Result<Tensor, AiError> {
        self.mask = Some(input.map(|x| if x > 0.0 { 1.0 } else { 0.0 }));
        Ok(input.map(|x| x.max(0.0)))
    }
    fn backward(&mut self, grad: &Tensor) -> Result<Tensor, AiError> {
        let mask = self
            .mask
            .as_ref()
            .ok_or_else(|| AiError::Training("backward called before forward".into()))?;
        grad.mul(mask)
    }
    fn update(&mut self, _optimizer: &mut Sgd) {}
}

pub struct Sequential {
    layers: Vec<Box<dyn Layer>>,
}
impl Sequential {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }
    pub fn add<L: Layer + 'static>(&mut self, layer: L) {
        self.layers.push(Box::new(layer));
    }
    pub fn forward(&mut self, input: &Tensor) -> Result<Tensor, AiError> {
        let mut x = input.clone();
        for layer in &mut self.layers {
            x = layer.forward(&x)?;
        }
        Ok(x)
    }
    pub fn backward(&mut self, grad: &Tensor) -> Result<Tensor, AiError> {
        let mut g = grad.clone();
        for layer in self.layers.iter_mut().rev() {
            g = layer.backward(&g)?;
        }
        Ok(g)
    }
    pub fn update(&mut self, optimizer: &mut Sgd) {
        for layer in &mut self.layers {
            layer.update(optimizer);
        }
    }
}
impl Default for Sequential {
    fn default() -> Self {
        Self::new()
    }
}

pub fn mse(prediction: &Tensor, target: &Tensor) -> Result<(f64, Tensor), AiError> {
    let diff = prediction.sub(target)?;
    let n = diff.data.len() as f64;
    let loss = diff.data.iter().map(|x| x * x).sum::<f64>() / n;
    Ok((loss, diff.scale(2.0 / n)))
}

pub struct Sgd {
    pub learning_rate: f64,
}
impl Sgd {
    pub fn new(learning_rate: f64) -> Self {
        Self { learning_rate }
    }
    fn update(&self, parameter: &mut Tensor, gradient: &Tensor) {
        for (p, g) in parameter.data.iter_mut().zip(&gradient.data) {
            *p -= self.learning_rate * g;
        }
    }
}

pub struct Trainer {
    pub epochs: usize,
    pub optimizer: Sgd,
}
impl Trainer {
    pub fn new(epochs: usize, learning_rate: f64) -> Self {
        Self {
            epochs,
            optimizer: Sgd::new(learning_rate),
        }
    }
    pub fn train(
        &mut self,
        model: &mut Sequential,
        inputs: &Tensor,
        targets: &Tensor,
    ) -> Result<Vec<f64>, AiError> {
        if inputs.rows != targets.rows {
            return Err(AiError::Training(
                "inputs and targets must have the same number of rows".into(),
            ));
        }
        let mut history = Vec::with_capacity(self.epochs);
        for _ in 0..self.epochs {
            let prediction = model.forward(inputs)?;
            let (loss, grad) = mse(&prediction, targets)?;
            model.backward(&grad)?;
            model.update(&mut self.optimizer);
            history.push(loss);
        }
        Ok(history)
    }
}

struct XorShift64 {
    state: u64,
}
impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x9e3779b97f4a7c15 } else { seed },
        }
    }
    fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }
    fn next_f64(&mut self) -> f64 {
        self.next() as f64 / u64::MAX as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matmul_works() {
        let a = Tensor::from_vec(2, 2, vec![1.0, 2.0, 3.0, 4.0]).unwrap();
        let b = Tensor::from_vec(2, 1, vec![5.0, 6.0]).unwrap();
        assert_eq!(a.matmul(&b).unwrap().data, vec![17.0, 39.0]);
    }

    #[test]
    fn dense_learns_simple_function() {
        let x = Tensor::from_vec(4, 1, vec![0.0, 1.0, 2.0, 3.0]).unwrap();
        let y = Tensor::from_vec(4, 1, vec![1.0, 3.0, 5.0, 7.0]).unwrap();
        let mut model = Sequential::new();
        model.add(Dense::new(1, 1, 42));
        let mut trainer = Trainer::new(500, 0.05);
        let history = trainer.train(&mut model, &x, &y).unwrap();
        assert!(history.last().unwrap() < history.first().unwrap());
    }

    #[test]
    fn relu_blocks_negative_values() {
        let x = Tensor::from_vec(1, 3, vec![-2.0, 0.0, 3.0]).unwrap();
        let mut relu = Relu::new();
        assert_eq!(relu.forward(&x).unwrap().data, vec![0.0, 0.0, 3.0]);
    }
}
