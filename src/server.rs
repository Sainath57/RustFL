/************************************************************************************/
//Server source code is contributed by Sai Pranavi Reddy Patlolla  & Sainath Talakanti
//Documentation is contributed by Sainath Talakanti
//Readme file is contributed by Sharvani Chelumalla

pub use actix_web::{get, post, web, App, HttpServer, Responder, HttpResponse};
pub use serde::{Deserialize, Serialize};
pub use log::info;
pub use tch::{nn, nn::Module, nn::OptimizerConfig, Tensor};
pub use std::sync::{Arc, Mutex};
pub use reqwest::Response;
use crate::secure_dp_utils::fed_avg_encrypted;

//Implemented by Sharvani Chelumalla
/// Struct to represent weight updates sent to the server.
#[derive(Debug, Serialize, Deserialize)]
pub struct WeightsUpdate {
    pub model_weights: Vec<String>,
    pub num_samples: usize,
    pub loss: f64,
    pub model_version: usize,
}

//Implemented by Sai Pranavi Reddy Patlolla
/// Global state for model version and client updates
pub struct AppState {
    pub aggregation_goal: usize,
    pub current_model_version: Mutex<usize>,
    pub client_updates: Mutex<Vec<WeightsUpdate>>,
    pub global_model: Mutex<nn::Sequential>,
}
//Implemented by Sai Pranavi Reddy Patlolla
impl AppState{
    /// Default global state if not defined by user
    pub fn default() -> Self{
        let vs = Arc::new(nn::VarStore::new(tch::Device::Cpu));
        let global_model = create_model(&vs.root());
        AppState {
            aggregation_goal: 1,
            current_model_version: Mutex::new(0),
            client_updates: Mutex::new(Vec::new()),
            global_model: Mutex::new(global_model)
        }
    }
}

//Implemented by Sharvani Chelumalla
/// A CNN construction using max-pooling and activation functions
pub fn create_model(vs: &nn::Path) -> nn::Sequential {
    nn::seq()
        .add(nn::conv2d(vs, 1, 32, 3, nn::ConvConfig::default()))
        .add_fn(|xs| xs.max_pool2d_default(2))
        .add_fn(|xs| xs.view([-1, 32 * 14 * 14]))
        .add(nn::linear(vs, 32 * 14 * 14, 128, Default::default()))
        .add_fn(|xs| xs.relu())
        .add(nn::linear(vs, 128, 10, Default::default()))
}

//Implemented by Sai Pranavi Reddy Patlolla
#[get("/get_model")]
/// Stores the global model weights such that client can fetch the global weights
pub async fn get_model(data: web::Data<AppState>) -> impl Responder {
    let _global_model = data.global_model.lock().unwrap();
    /*let model_state_dict = global_model
        .parameters()
        .iter()
        .map(|(key, value)| (key.clone(), value.to_kind(tch::Kind::Float).to_vec()))
        .collect::<Vec<_>>();
     */
    let model_state_dict = "Model State Dict".to_string();

    HttpResponse::Ok().json(serde_json::json!({
        "model_state_dict": model_state_dict,
        "model_version": *data.current_model_version.lock().unwrap()
    }))
}

//Implemented by Sai Pranavi Reddy Patlolla
#[post("/update_model")]
/// Updates the global model each time client sends the updated version of weights
pub async fn update_model(update: web::Json<WeightsUpdate>, data: web::Data<AppState>) -> impl Responder {
    info!("Received model update from client with loss: {}",update.loss);

    let mut client_updates = data.client_updates.lock().unwrap();
    client_updates.push(update.into_inner());

    if client_updates.len() >= data.aggregation_goal {
        let selected_clients = client_updates.split_off(0); // Select clients for aggregation
        let encrypted_weights_list = selected_clients
            .iter()
            .map(|client| client.model_weights.clone())
            .collect::<Vec<_>>();

        let aggregated_encrypted_weights = fed_avg_encrypted(encrypted_weights_list);
        info!("Aggregation is successful!");

        let mut current_version = data.current_model_version.lock().unwrap();
        info!("Global model updated, Version: {}",current_version);
        *current_version += 1;

        HttpResponse::Ok().json(serde_json::json!({
            "message": "Global model updated with encrypted weights",
            "encrypted_model_weights": aggregated_encrypted_weights,
            "model_version": *current_version
        }))
    } else {
        HttpResponse::Ok().json(serde_json::json!({
            "message": format!(
                "Waiting for more client updates. Received {}/{} updates",
                client_updates.len(),
                data.aggregation_goal
            )
        }))
    }
}

//Tests
//Unit tests are contributed by Sharvani Chelumalla & Sai Pranavi Reddy Patlolla
#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App};
    use tch::{Device, nn};
    use serde_json::json;
    use actix_web::http;

    // Test for get_model function (Asynchronous)
    #[tokio::test]
    async fn test_get_model() {
        let app_state = web::Data::new(AppState::default());
        let mut app = test::init_service(
            App::new()
                .app_data(app_state.clone())
                .service(get_model)
        ).await;

        // Send a GET request to the '/get_model' endpoint
        let req = test::TestRequest::get().uri("/get_model").to_request();
        let response = test::call_service(&mut app, req).await;

        // Assert that the response is 200 OK
        assert_eq!(response.status(), http::StatusCode::OK);

        // Parse the response body as JSON and check the contents
        let response_body: serde_json::Value = test::read_body_json(response).await;
        assert!(response_body["model_state_dict"].is_string());
        assert!(response_body["model_version"].is_u64());
    }

    // Test for update_model function (Asynchronous)
    #[tokio::test]
    async fn test_update_model() {
        let app_state = web::Data::new(AppState::default());
        let mut app = test::init_service(
            App::new()
                .app_data(app_state.clone())
                .service(update_model)
        ).await;

        // Create a sample WeightsUpdate
        let weights_update = WeightsUpdate {
            model_weights: vec!["weight1".to_string(), "weight2".to_string()],
            num_samples: 100,
            loss: 0.25,
            model_version: 1,
        };

        // Send a POST request to the '/update_model' endpoint with the WeightsUpdate
        let req = test::TestRequest::post()
            .uri("/update_model")
            .set_json(&weights_update)
            .to_request();
        let response = test::call_service(&mut app, req).await;

        // Assert that the response is 200 OK
        assert_eq!(response.status(), http::StatusCode::OK);

        // Parse the response body as JSON and check the contents
        let response_body: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(response_body["message"], "Global model updated with encrypted weights");
        assert!(response_body["encrypted_model_weights"].is_array());
        assert!(response_body["model_version"].is_u64());
    }
}