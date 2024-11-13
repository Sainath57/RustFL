//use std::collections::HashMap;
use std::fmt::Debug;
use log::{error, info, warn};
use reqwest::Client;
use serde_json::Value;
use tch::{kind, nn::{self, Conv2D, Linear, Module, Optimizer, OptimizerConfig, Sgd, VarStore}, Device, Kind, Tensor};
use serde::{Deserialize, Serialize};
use rand_distr::{Normal,Distribution};
use rand::{thread_rng,Rng};
use fernet::Fernet;


// Struct to represent weight updates sent to the server.
#[derive(Serialize, Deserialize)]
pub struct WeightsUpdate {
    // Weights of the model.
    model_weights: Vec<String>,
    num_samples: usize,
    loss: f64,
    model_version: usize,
}

pub struct Config {
    learning_rate: f64,
    batch_size: usize,
    noise_level: f64,
    num_rounds: usize,
    sensitivity: f64,
    epsilon: f64,
}

impl Config {
    pub fn new() -> Self {
        Config {
            learning_rate: 0.001,
            batch_size: 64,
            noise_level: 0.1,  // This can be adjusted for DP
            num_rounds: 3,
            sensitivity: 1.0,  // Sensitivity of the function (adjust as necessary)
            epsilon: 0.5,  // Privacy budget (adjust as necessary)
        }
    }
}

// Define the SimpleCNN model structure.
#[derive(Debug)]
pub struct SimpleCNN {
    conv1: Conv2D,
    //conv2: Conv2D,
    fc1: Linear,
    fc2: Linear,
}

// Implementation of the SimpleCNN model.
/*impl SimpleCNN {
    // Create a new instance of SimpleCNN.
    fn new(vs: &nn::Path) -> SimpleCNN {
        let conv1 = nn::conv2d(vs, 1, 32, 3, nn::ConvConfig {
            stride: 1,
            padding: 1,
            ..Default::default()
        });
        let conv2 = nn::conv2d(vs, 32, 64, 3, nn::ConvConfig {
            stride: 1,
            padding: 1,
            ..Default::default()
        });
        let fc1 = nn::linear(vs, 64 * 7 * 7, 128, Default::default());
        let fc2 = nn::linear(vs, 128, 10, Default::default());
        SimpleCNN {
            conv1,
            conv2,
            fc1,
            fc2,
        }
    }
}

// Implementation of the nn::Module trait for SimpleCNN.
impl nn::Module for SimpleCNN {
    fn forward(&self, xs: &Tensor) -> Tensor {
        let xs = xs.apply(&self.conv1)
            .relu()
            .max_pool2d(&[2, 2], &[2, 2], &[0, 0], &[1, 1], false)
            .apply(&self.conv2)
            .relu()
            .max_pool2d(&[2, 2], &[2, 2], &[0, 0], &[1, 1], false)
            .view([-1, 64 * 7 * 7])
            .apply(&self.fc1)
            .relu()
            .apply(&self.fc2);

        xs
    }
}*/

impl SimpleCNN {
    pub fn new(vs: &nn::Path) -> SimpleCNN {
        let conv1 = nn::conv2d(vs, 1, 32, 3, Default::default());
        //let conv2 = nn::conv2d(vs, 32, 64, 3, Default::default());
        let fc1 = nn::linear(vs, 32*13*13, 128, Default::default());
        let fc2 = nn::linear(vs, 128, 10, Default::default());

        SimpleCNN { conv1,
            //conv2,
            fc1,
            fc2 }
    }

    pub fn forward(&self, xs: &Tensor) -> Tensor {
        let xs = xs.view([-1, 1, 28, 28]); // Assuming batch size can vary
        //info!("Input shape: {:?}", xs.size());

        let xs = xs.apply(&self.conv1).relu();
        //info!("After conv1: {:?}", xs.size());

        let xs = xs.max_pool2d_default(2);
        //info!("After max_pool2d (1): {:?}", xs.size());

        // Update the view size based on Python output
        let xs = xs.view([-1, 32 * 13 * 13]);
        //info!("Flattened shape: {:?}", xs.size());

        let xs = xs.apply(&self.fc1).relu();
        //info!("After fc1: {:?}", xs.size());

        let xs = xs.apply(&self.fc2);
        //info!("After fc2: {:?}", xs.size());

        xs
    }
}

// Function to train the local model.
pub fn train_local_model(
    train_loader: &Vec<(Tensor, Tensor)>,
    model: &mut SimpleCNN,
    optimizer: &mut Optimizer,
    criterion: &dyn Fn(&Tensor, &Tensor) -> Tensor,
    device: Device
) -> (f64, Vec<f64>) {
    //model.train();
    let mut running_loss = 0.0;
    info!("Training");

    for (batch_idx, (data, target)) in train_loader.iter().enumerate() {

        let data = data.to(device);//.view([-1,1,28,28]); ON HOLD
        //info!("Data shape: {:?}", data.size());
        let target = target.to(device);
        //info!("Target shape: {:?}", target.size());
        optimizer.zero_grad();

        /*println!("********************************");
        println!();
        println!("{}",data);
        println!();
        println!("********************************");
         */

        // Forward pass
        let output = model.forward(&data);
        //info!("Output shape: {:?}", output.size());
        let loss = criterion(&output, &target);
        loss.backward();
        optimizer.step();

        running_loss += loss.double_value(&[]);

        // Log the loss every 100 batches.
        if batch_idx % 100 == 0 {
            info!("Batch {}/{}, Loss: {}", batch_idx, train_loader.len(), loss.double_value(&[]));
        }
    }

    let avg_loss = running_loss / train_loader.len() as f64;
    info!("Average Loss: {}", avg_loss);
    /*******************
    (avg_loss, model.state_dict())
    *******************/
    (avg_loss,vec![1f64,2f64]) // Added in Next Release

}

pub struct DPMechanism{
    epsilon:f64,
    sensitivity:f64
}
impl DPMechanism{
    pub fn new(epsilon:f64,sensitivity:f64)-> DPMechanism{
        DPMechanism{
            epsilon,
            sensitivity
        }
    }
    pub fn add_noise(&self, weights: &Vec<f64>) -> Vec<f64> {
        let noise_std = self.sensitivity / self.epsilon;
        let normal_dist = Normal::new(0.0, noise_std).unwrap();
        let mut rng = thread_rng();

        // Adding Gaussian noise to each weight
        weights
            .iter()
            .map(|&weight| weight + normal_dist.sample(&mut rng))
            .collect()
    }
}

pub fn secret_share_weights(weights: Vec<f64>, num_shares: usize, threshold: usize, _noise_level: f64) -> Vec<Vec<f64>> {
    // Create a vector of vectors to hold shares for each shareholder
    let mut shares = vec![vec![]; num_shares];
    let mut rng = thread_rng();

    for &weight in &weights {
        // Generate random coefficients for the polynomial of degree (threshold - 1)
        let mut coeffs: Vec<f64> = (0..(threshold - 1)).map(|_| rng.gen_range(0.0..100.0)).collect();  // Generate random coefficients

        // Include the secret as the constant term of the polynomial
        coeffs.insert(0, weight);

        for i in 0..num_shares {
            // Calculate the share by evaluating the polynomial at (i + 1)
            let x = (i + 1) as f64;
            let share: f64 = coeffs.iter()
                .enumerate()
                //.map(|(idx, &coeff)| coeff * x.powi(idx as i32))  // x^idx * coeff
                //.sum();
                .fold(0.0, |acc, (idx, &coeff)| acc+coeff *(x.powi(idx as i32)));
            shares[i].push(share);  // Append the share to the shares vector
        }
    }

    shares  // Return the shares as a vector of vectors
}

pub fn encrypt_share(share: &str, key: &str) -> Result<Vec<u8>,String> {
    // Create a Fernet instance from the provided key
    let fernet = Fernet::new(key).ok_or("Invalid Key");

    // Encrypt the share
    let encrypted_share = fernet?.encrypt(share.as_bytes());

    Ok(encrypted_share.into())
}


// Asynchronously send local model weights to the server.
pub async fn send_local_model_weights(
    weights: Vec<f64>,
    loss_value: f64,
    model_version: usize,
    model: &SimpleCNN,
    encryption_key: &str,
    dp_mechanism: &DPMechanism,
    _device: Device
) {
    let model_weights_list_noisy:Vec<f64> = dp_mechanism.add_noise(&weights);
    let shared_weights = secret_share_weights(model_weights_list_noisy,3,2,Config::new().noise_level);
    let encrypted_shares = shared_weights
        .iter()
        .map(|share| encrypt_share(&format!("{:?}",share),encryption_key))
        .map(|encrypted_bytes| String::from_utf8(encrypted_bytes.unwrap()).unwrap())
        .collect();

    let client_updates = WeightsUpdate {
        model_weights: encrypted_shares,
        num_samples: weights.len() as usize,
        loss: loss_value as f64,
        model_version,
    };

    let url = "http://0.0.0.0:8081/update_model";

    let client = Client::new();
    // Send the weight update as a JSON payload.
    let response = client.post(url)
        .json(&client_updates)
        .send()
        .await.unwrap();

    if response.status().is_success() {
        info!("Model update successful");
    } else if response.status().as_u16() == 409 {
        warn!("Model version mismatch. Fetching the latest model.");
        fetch_global_model(model).await.unwrap(); // Fetch the latest model if there's a version mismatch.
    } else {
        error!("Failed to send model update: {}", response.status());
    }
}

// Asynchronously fetch the global model from the server.
pub async fn fetch_global_model(model: &SimpleCNN) -> Result<&SimpleCNN, reqwest::Error> {
    let client = Client::new();
    let url = "http://0.0.0.0:8081/get_model";

    // Send GET request to fetch the global model.
    let response = client.get(url).send().await?;

    if response.status().is_success() {
        let data: Value = response.json().await?;

        /*****************************************************************************
        //Tp find keys in data
                if let Value::Object(map) = data {
                    // Get the keys
                    let keys: Vec<String> = map.keys().cloned().collect();

                    // Print the keys
                    for key in keys {
                        println!("{}", key);
                    }
                } else {
                    println!("Response is not a JSON object.");
                }
        *****************************************************************************************/

        let _global_model_weights = data.get("model_state_dict").unwrap();

        // Load the fetched global model weights into the model.
        /*******************
         model.load_state_dict(global_model_weights, false)?; // Added in next release
        *********************/
        //model.load_state_dict(global_model_weights, false)?; // Added in next release
        info!("Fetched global model");
    } else {
        error!("Failed to fetch global model");
    }
    Ok(model)
}

// Asynchronously start the training process.
pub async fn start_training(
    train_loader: Vec<(Tensor, Tensor)>,
    encryption_key: &str,
    model: &mut SimpleCNN,
    optimizer: &mut Optimizer,
    criterion: &dyn Fn(&Tensor, &Tensor) -> Tensor,
    device: Device
) {

    let dp_mechanism = DPMechanism::new(Config::new().epsilon, Config::new().sensitivity);

    let url = "http://0.0.0.0:8081/get_model";
    let client = Client::new();

    // Fetch initial model version.
    let initial_response = client.get(url).send().await.unwrap();
    let data: Value = initial_response.json().await.unwrap();
    let model_version = data.get("model_version")
                                .and_then(|v| v.as_f64())
                                .map(|v| v as usize)
                                .expect("model_version is not a valid Integer");
    // Training for a defined number of rounds.
    for round_num in 0..Config::new().num_rounds {
        info!("Round {}", round_num + 1);
        fetch_global_model(model).await.unwrap();

        // Train the local model and send weights to the server.
        let (loss_value, trained_weights) = train_local_model(&train_loader, model, optimizer, criterion, device);
        send_local_model_weights(trained_weights, loss_value, model_version.clone(), model, encryption_key,&dp_mechanism,device).await;
    }
    info!("Training completed for 3 rounds");
}

// Function to load and normalize training data.
pub fn get_train_data(data_dir:String) -> Vec<(Tensor, Tensor)> {
    #[derive(Debug)]
    struct Normalize {
        mean: Tensor,
        stddev: Tensor,
    }

    impl Normalize {
        fn new(mean: Tensor, stddev: Tensor) -> Self {
            Normalize { mean, stddev }
        }
    }

    impl Module for Normalize {
        fn forward(&self, input: &Tensor) -> Tensor {
            ((input.to_kind(Kind::Float) / 255.0) - &self.mean) / &self.stddev
        }
    }

    // Define normalization parameters.
    let mean = Tensor::from_slice(&[0.1307]).to_kind(Kind::Float);
    let stddev = Tensor::from_slice(&[0.3081]).to_kind(Kind::Float);
    let transform = Normalize::new(mean, stddev);

    // Load MNIST dataset.
    let dataset = tch::vision::mnist::load_dir(data_dir).unwrap();

    /**************************************************************************************************
        println!("Loaded MNIST dataset with {} training images.", dataset.train_images.size()[0]);

        // Normalize and subset training dataset.
        let train_dataset_images = transform.forward(&dataset.train_images);
        let train_dataset_labels = dataset.train_labels.to_kind(Kind::Int64);
        println!("Images shape after normalization: {:?}", train_dataset_images.size());
        println!("Labels shape: {:?}", train_dataset_labels.size());

        // Ensure we have enough data to create a subset.
        let subset_size = 10000;
        let total_images = train_dataset_images.size()[0];
        let total_labels = train_dataset_labels.size()[0];

        if total_images < subset_size || total_labels < subset_size {
            panic!("Not enough data in dataset: images {}, labels {}", total_images, total_labels);
        }

    *************************************************************************************************/

    // Normalize and subset training dataset.
    let train_dataset_images = transform.forward(&dataset.train_images);
    let train_dataset_labels = dataset.train_labels.to_kind(kind::Kind::Int64);
    let subset_train_dataset_images = train_dataset_images.narrow(0, 0, 10000);
    let subset_train_dataset_labels = train_dataset_labels.narrow(0, 0, 10000);

    // Create a vector of tuples (image, label) for training data.
    let mut train_dataset = Vec::new();
    for i in 0..subset_train_dataset_images.size()[0] {
        let image = subset_train_dataset_images.get(i).squeeze();
        let label = subset_train_dataset_labels.get(i).squeeze();
        let label = label.view([-1]);
        train_dataset.push((image, label));
    }
    let mut batches = Vec::new();

    for chunk in train_dataset.chunks(Config::new().batch_size) {
        if chunk.is_empty() {
            continue; // Skip empty chunks
        }

        let batch_dataset_images = Tensor::cat(&chunk.iter().map(|(d, _)| d.copy()).collect::<Vec<_>>(), 0);
        let batch_dataset_labels = Tensor::cat(&chunk.iter().map(|(_, t)| t.copy()).collect::<Vec<_>>(), 0);

        // Log shapes
        //info!("Batch data shape: {:?}", batch_dataset_images.size());
        //info!("Batch target shape: {:?}", batch_dataset_labels.size());

        batches.push((batch_dataset_images, batch_dataset_labels));
    }

    batches
    //train_dataset
}


// Main function to initialize and start the training process.
#[tokio::main]
pub async fn main() {
    env_logger::init();
    let device = if tch::Cuda::is_available() { Device::Cuda(0) } else { Device::Cpu };
    let vs = VarStore::new(device);
    let mut simple_cnn_model = SimpleCNN::new(&vs.root());
    let mut optimizer = Sgd::default().build(&vs, Config::new().learning_rate).unwrap();

    // Define the loss function.
    let criterion = |output: &Tensor, target: &Tensor| {
        output
            .cross_entropy_for_logits(target)
            .mean(Kind::Float)
    };

    // Load the training data.
    let train_loader = get_train_data("mnist_data/MNIST/raw".to_string());

    let encryption_key = Fernet::generate_key();

    // Start the training process asynchronously.
    start_training(train_loader, encryption_key.as_str(),&mut simple_cnn_model, &mut optimizer, &criterion, device).await;

    info!("Model training has been completed.");
}

/******************************************************************************************
Arm architecture required
latest libtorch file need to be downloaded from official pytorch website
pytorch 2.5.0 version is required

for OpenSSL error: follow the steps
        Download openssl-3.4.0.tar.gz from github and extract it
        In the terminal, follow:
        cd /Absolute/path/to/openssl-3.4.0
        ./config --prefix=$HOME/openssl --openssldir=$HOME/openssl
        make
        make install
        export OPENSSL_DIR=$HOME/openssl
        export OPENSSL_LIB_DIR=$OPENSSL_DIR/lib
        export OPENSSL_INCLUDE_DIR=$OPENSSL_DIR/include
        export PKG_CONFIG_PATH=$OPENSSL_LIB_DIR/pkgconfig
        source ~/.bashrc
        ls $OPENSSL_LIB_DIR
        ls $OPENSSL_INCLUDE_DIR
        cd /Absolute/path/to/RustFL
        cargo clean
        cargo build

For torch not found error, follow following in terminal:
        python3 --version #verify the version and update
        python3 -m pip install --upgrade pip
        pip install torch torchvision torchaudio --extra-index-url https://download.pytorch.org/whl/cu117
        source /path/to/venv/bin/activate
        export LIBTORCH_USE_PYTORCH=1

For .dyld file not found error:
        export DYLD_LIBRARY_PATH=/Absolute/path/to/libtorch/lib

To run client code with logInfo:
        RUST_LOG=info cargo run --bin client

For Amd architecture:
    Pytorch 2.2.0 and tch 0.15.0 and appropriate libtorch file is require required
 */