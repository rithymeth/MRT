use mrt_ai::{Dense, Sequential, Tensor, Trainer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let x = Tensor::from_vec(4, 1, vec![0.0, 1.0, 2.0, 3.0])?;
    let y = Tensor::from_vec(4, 1, vec![1.0, 3.0, 5.0, 7.0])?;

    let mut model = Sequential::new();
    model.add(Dense::new(1, 1, 7));

    let mut trainer = Trainer::new(500, 0.05);
    let history = trainer.train(&mut model, &x, &y)?;

    println!("MRT-AI training complete");
    println!("initial loss: {:.6}", history[0]);
    println!("final loss:   {:.6}", history[history.len() - 1]);

    let prediction = model.forward(&x)?;
    println!("predictions: {:?}", prediction.data);
    Ok(())
}
