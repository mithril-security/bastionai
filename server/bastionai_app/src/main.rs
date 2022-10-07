use env_logger::Env;
use log::info;
use std::collections::HashMap;

use std::{
    hash::{Hash, Hasher},
    collections::hash_map::DefaultHasher};
use std::ffi::CString;
use std::fs;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use std::{fs::File, io::Read};
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Identity;
use tonic::transport::ServerTlsConfig;

use ring::digest;

use tonic::{transport::Server, Request, Response, Status, Streaming};
use uuid::Uuid;

use bastionai_learning::{data::Dataset, nn::Module};

mod remote_torch {
    tonic::include_proto!("remote_torch");
}
use remote_torch::remote_torch_server::{RemoteTorch, RemoteTorchServer};
use remote_torch::{
    Chunk, ClientInfo, Devices, Empty, Metric, Optimizers, Reference, References, TestConfig,
    TrainConfig,
};

mod telemetry;
use telemetry::TelemetryEventProps;

mod storage;
use storage::Artifact;

mod utils;
use utils::*;

mod learning;
use learning::*;

mod serialization;
use serialization::*;

use bastionai_learning::serialization::SizedObjectsBytes;
use bastionai_common::auth::{auth_interceptor, setup_jwt};

//<!--Attestation Deps -->
use sha2::{Sha256, Digest};
mod attestation_lib;
use attestation_lib::*;

mod attestation {
    tonic::include_proto!("attestation");
}

use attestation::attestation_server::{Attestation,AttestationServer};
use attestation::{
    ReportRequest, ReportResponse,
};

/// The server's state
struct BastionAIServer {
    modules: RwLock<HashMap<String, Artifact<Module>>>,
    datasets: RwLock<HashMap<String, Artifact<Dataset>>>,
    runs: RwLock<HashMap<Uuid, Arc<RwLock<Run>>>>,
}

impl BastionAIServer {
    pub fn new() -> Self {
        BastionAIServer {
            modules: RwLock::new(HashMap::new()),
            datasets: RwLock::new(HashMap::new()),
            runs: RwLock::new(HashMap::new()),
        }
    }
}

#[tonic::async_trait]
impl Attestation for BastionAIServer {
    async fn client_report_request(&self, request: Request<ReportRequest>) -> Result<Response<ReportResponse>,Status>
    {

        let nonce = request.into_inner().nonce;
        let server_cert = fs::read("tls/host_server.pem");
        
        let mut hasher = Sha256::new();
        let data:Vec<u8> = match server_cert {
            Ok(mut cert) => {let mut nonce_bytes = nonce.to_vec();
                            nonce_bytes.append(&mut cert); 
                            nonce_bytes},
            _ => nonce.to_vec(),
        };
            
        hasher.update(data);
        let report_input_hash = hasher.finalize();
        
        let report_certs = get_report(report_input_hash.to_vec()).await.unwrap();

        let server_cert_unwrapped = fs::read("tls/host_server.pem")?;

        Ok(Response::new(ReportResponse{
            report: report_certs.get("report").unwrap().to_vec(),
            server_cert : server_cert_unwrapped,
            signature_algo: report_certs.get("signature_algo").unwrap().to_vec(),
            cert_chain: report_certs.get("cert_chain").unwrap().to_vec(),
            vcek_cert: report_certs.get("vcek_cert").unwrap().to_vec(),
        }))
    }
}


#[tonic::async_trait]
impl RemoteTorch for BastionAIServer {
    type FetchDatasetStream = ReceiverStream<Result<Chunk, Status>>;
    type FetchModuleStream = ReceiverStream<Result<Chunk, Status>>;

    async fn send_dataset(
        &self,
        request: Request<Streaming<Chunk>>,
    ) -> Result<Response<Reference>, Status> {
        let start_time = Instant::now();

        let artifact: Artifact<SizedObjectsBytes> = unstream_data(request.into_inner()).await?;

        let (dataset_hash, dataset_size) = {
            let lock = artifact.data.read().unwrap();
            let data = lock.get();
            let hash = hex::encode(digest::digest(&digest::SHA256, &data).as_ref());
            (hash, data.len())
        };

        let dataset: Artifact<Dataset> = tcherror_to_status((artifact).deserialize())?;
        let name = dataset.name.clone();
        let description = dataset.description.clone();
        let meta = dataset.meta.clone();
        let client_info = dataset.client_info.clone();

        self.datasets
            .write()
            .unwrap()
            .insert(dataset_hash.clone(), dataset);

        let elapsed = start_time.elapsed();
        info!(
        target: "BastionAI",
            
            "Upload Dataset successful in {}ms", elapsed.as_millis());

        telemetry::add_event(
            TelemetryEventProps::SendDataset {
                dataset_name: Some(name.clone()),
                dataset_size,
                time_taken: elapsed.as_millis() as f64,
                dataset_hash: Some(dataset_hash.clone())
            },
            client_info,
        );
        Ok(Response::new(Reference {
            identifier: format!("{}", dataset_hash),
            name,
            description,
            meta,
        }))
    }

    async fn send_model(
        &self,
        request: Request<Streaming<Chunk>>,
    ) -> Result<Response<Reference>, Status> {
        let start_time = Instant::now();

        let artifact: Artifact<SizedObjectsBytes> = unstream_data(request.into_inner()).await?;

        let (model_hash, model_size) = {
            let lock = artifact.data.read().unwrap();
            let data = lock.get();
            let hash = hex::encode(digest::digest(&digest::SHA256, &data).as_ref());
            (hash, data.len())
        };

        let module: Artifact<Module> = tcherror_to_status(artifact.deserialize())?;
        let name = module.name.clone();
        let description = module.description.clone();
        let meta = module.meta.clone();
        let client_info = module.client_info.clone();

        self.modules
            .write()
            .unwrap()
            .insert(model_hash.clone(), module);
        let elapsed = start_time.elapsed();

        info!(
        target: "BastionAI",
            
            "Upload Model successful in {}ms", elapsed.as_millis());

        telemetry::add_event(
            TelemetryEventProps::SendModel {
                model_name: Some(name.clone()),
                model_hash: Some(model_hash.clone()),
                model_size,
                time_taken: elapsed.as_millis() as f64,
            },
            client_info,
        );
        Ok(Response::new(Reference {
            identifier: format!("{}", model_hash),
            name,
            description,
            meta,
        }))
    }

    async fn fetch_dataset(
        &self,
        request: Request<Reference>,
    ) -> Result<Response<Self::FetchDatasetStream>, Status> {
        let identifier = request.into_inner().identifier;
        let serialized = {
            let datasets = self.datasets.read().unwrap();
            let artifact = datasets
                .get(&identifier)
                .ok_or(Status::not_found("Dataset not found"))?;
            tcherror_to_status(artifact.serialize())?
        };

        Ok(stream_data(serialized, 4_194_285, "Dataset".to_string()).await)
    }

    async fn fetch_module(
        &self,
        request: Request<Reference>,
    ) -> Result<Response<Self::FetchModuleStream>, Status> {
        let identifier = request.into_inner().identifier;
        let serialized = {
            let modules = self.modules.read().unwrap();
            let artifact = modules
                .get(&identifier)
                .ok_or(Status::not_found("Module not found"))?;
            tcherror_to_status(artifact.serialize())?
        };

        Ok(stream_data(serialized, 4_194_285, "Model".to_string()).await)
    }

    async fn delete_dataset(&self, request: Request<Reference>) -> Result<Response<Empty>, Status> {
        let identifier = request.into_inner().identifier;
        self.datasets.write().unwrap().remove(&identifier);
        Ok(Response::new(Empty {}))
    }

    async fn delete_module(&self, request: Request<Reference>) -> Result<Response<Empty>, Status> {
        let identifier = request.into_inner().identifier;
        self.modules.write().unwrap().remove(&identifier);
        Ok(Response::new(Empty {}))
    }

    async fn train(&self, request: Request<TrainConfig>) -> Result<Response<Reference>, Status> {
        let config = request.into_inner();
        let dataset_id = config
            .dataset
            .clone()
            .ok_or(Status::invalid_argument("Invalid dataset reference"))?
            .identifier;
        let module_id = config
            .model
            .clone()
            .ok_or(Status::invalid_argument("Invalid module reference"))?
            .identifier;
        let device = parse_device(&config.device)?;
        let (module, client_info) = {
            let modules = self.modules.read().unwrap();
            let module = modules
                .get(&module_id)
                .ok_or(Status::not_found("Module not found"))?;
            (Arc::clone(&module.data), module.client_info.clone())
        };

        let dataset = {
            let datasets = self.datasets.read().unwrap();
            let dataset = datasets
                .get(&dataset_id)
                .ok_or(Status::not_found("Dataset not found"))?;
            Arc::clone(&dataset.data)
        };

        let identifier = Uuid::new_v4();
        self.runs
            .write()
            .unwrap()
            .insert(identifier, Arc::new(RwLock::new(Run::Pending)));
        let run = Arc::clone(self.runs.read().unwrap().get(&identifier).unwrap());
        module_train(module, dataset, run, config, device, module_id, dataset_id, client_info);
        Ok(Response::new(Reference {
            identifier: format!("{}", identifier),
            name: format!("Run #{}", identifier),
            description: String::from(""),
            meta: Vec::new(),
        }))
    }

    async fn test(&self, request: Request<TestConfig>) -> Result<Response<Reference>, Status> {
        let config = request.into_inner();
        let dataset_id = config
            .dataset
            .clone()
            .ok_or(Status::invalid_argument("Invalid dataset reference"))?
            .identifier;
        let module_id = config
            .model
            .clone()
            .ok_or(Status::invalid_argument("Invalid dataset reference"))?
            .identifier;
        let device = parse_device(&config.device)?;
        let (module, client_info) = {
            let modules = self.modules.read().unwrap();
            let module = modules
                .get(&module_id)
                .ok_or(Status::not_found("Module not found"))?;
            (Arc::clone(&module.data), module.client_info.clone())
        };

        let dataset = {
            let datasets = self.datasets.read().unwrap();
            let dataset = datasets
                .get(&dataset_id)
                .ok_or(Status::not_found("Dataset not found"))?;
            Arc::clone(&dataset.data)
        };

        let identifier = Uuid::new_v4();
        self.runs
            .write()
            .unwrap()
            .insert(identifier, Arc::new(RwLock::new(Run::Pending)));
        let run = Arc::clone(self.runs.read().unwrap().get(&identifier).unwrap());
        module_test(module, dataset, run, config, device, module_id, dataset_id, client_info);
        Ok(Response::new(Reference {
            identifier: format!("{}", identifier),
            name: format!("Run #{}", identifier),
            description: String::from(""),
            meta: Vec::new(),
        }))
    }

    async fn available_models(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<References>, Status> {
        let list = self
            .modules
            .read()
            .unwrap()
            .iter()
            .map(|(k, v)| Reference {
                identifier: format!("{}", k),
                name: v.name.clone(),
                description: v.description.clone(),
                meta: v.meta.clone(),
            })
            .collect();

        Ok(Response::new(References { list }))
    }

    async fn available_datasets(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<References>, Status> {
        let list = self
            .datasets
            .read()
            .unwrap()
            .iter()
            .map(|(k, v)| Reference {
                identifier: format!("{}", k),
                name: v.name.clone(),
                description: v.description.clone(),
                meta: v.meta.clone(),
            })
            .collect();

        Ok(Response::new(References { list }))
    }

    async fn available_devices(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Devices>, Status> {
        let mut list = vec![String::from("cpu")];
        if tch::Cuda::is_available() {
            list.push(String::from("gpu"));
            for index in 0..tch::Cuda::device_count() {
                list.push(format!("cuda:{}", index));
            }
        }

        Ok(Response::new(Devices { list }))
    }

    async fn available_optimizers(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Optimizers>, Status> {
        let list = vec!["SGD", "Adam"].iter().map(|v| v.to_string()).collect();
        Ok(Response::new(Optimizers { list }))
    }

    async fn get_metric(&self, request: Request<Reference>) -> Result<Response<Metric>, Status> {
        let identifier = Uuid::parse_str(&request.into_inner().identifier)
            .map_err(|_| Status::invalid_argument("Invalid run reference"))?;

        match &*self
            .runs
            .read()
            .unwrap()
            .get(&identifier)
            .unwrap()
            .read()
            .unwrap()
        {
            Run::Pending => Err(Status::out_of_range("Run has not started.")),
            Run::Ok(m) => Ok(Response::new(m.clone())),
            Run::Error(e) => Err(Status::internal(e.message())),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let logo_str: &str = include_str!("../logo.txt");
    let version_str: String = format!("VERSION : {}", env!("CARGO_PKG_VERSION"));
    let text_size: usize = 58;
    println!("{}\n", logo_str);
    fill_blank_and_print("BastionAI - SECURE AI TRAINING SERVER", text_size);
    fill_blank_and_print("MADE BY MITHRIL SECURITY", text_size);
    fill_blank_and_print(
        "GITHUB: https://github.com/mithril-security/bastionai",
        text_size,
    );
    fill_blank_and_print(&version_str, text_size);

    // Identity for untrusted (non-attested) communication
    let server_cert = fs::read("tls/host_server.pem")?;
    let server_key = fs::read("tls/host_server.key")?;
    let server_identity = Identity::from_pem(&server_cert, &server_key);

    setup_jwt();

    let server = BastionAIServer::new();
    let attestation_server = BastionAIServer::new();

    let mut file = File::open("config.toml")?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let network_config: bastionai_common::NetworkConfig = toml::from_str(&contents)?;

    let platform: CString = CString::new(format!("{}", whoami::platform())).unwrap();
    let uid: CString = {
        let mut hasher = DefaultHasher::new();
        whoami::username().hash(&mut hasher);
        whoami::hostname().hash(&mut hasher);
        platform.hash(&mut hasher);
        CString::new(format!("{:X}", hasher.finish())).unwrap()
    };

    if std::env::var("BASTIONAI_DISABLE_TELEMETRY").is_err() {
        telemetry::setup(platform.into_string().unwrap(), uid.into_string().unwrap())?;
    }
    else {
        info!(
            target: "BastionAI",
            "Telemetry is disabled.")
    }
    telemetry::add_event(TelemetryEventProps::Started {}, None);
    info!(
        target: "BastionAI",
        "BastionAI listening on {}",
        network_config.client_to_enclave_untrusted_socket()?
    );
    Server::builder()
        .tls_config(ServerTlsConfig::new().identity(server_identity))?
        .add_service(AttestationServer::new(attestation_server))
        .add_service(RemoteTorchServer::with_interceptor(server, auth_interceptor))
        .serve(network_config.client_to_enclave_untrusted_socket()?)
        .await?;

    Ok(())
}
