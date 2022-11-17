use polars::prelude::*;
use serde_json;
use std::{
    collections::HashMap,
    error::Error,
    fmt::Debug,
    sync::{Arc, RwLock},
};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{transport::Server, Request, Response, Status, Streaming};
use uuid::Uuid;

pub mod grpc {
    tonic::include_proto!("bastionlab");
}
use grpc::{
    bastion_lab_server::{BastionLab, BastionLabServer},
    Chunk, Empty, Query, ReferenceList, ReferenceRequest, ReferenceResponse,
};

mod serialization;
use serialization::*;

mod composite_plan;
use composite_plan::*;

mod visitable;


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


#[tonic::async_trait]
impl Attestation for BastionLabState {
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



#[derive(Debug, Clone)]
pub struct DataFrameArtifact {
    dataframe: DataFrame,
    fetchable: bool,
    query_details: String,
}

impl DataFrameArtifact {
    pub fn new(df: DataFrame) -> Self {
        DataFrameArtifact { dataframe: df, fetchable: false, query_details: String::from("uploaded dataframe") }
    }
}

#[derive(Debug, Default)]
pub struct BastionLabState {
    dataframes: Arc<RwLock<HashMap<String, DataFrameArtifact>>>,
}

impl BastionLabState {
    fn new() -> Self {
        Self {
            dataframes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn get_df(&self, identifier: &str) -> Result<DataFrame, Status> {
        let dfs = self.dataframes.read().unwrap();
        let artifact = dfs
            .get(identifier)
            .ok_or(Status::not_found(format!(
                "Could not find dataframe: identifier={}",
                identifier
            )))?;
        if !artifact.fetchable {
            println!(
                "=== A user request has been rejected ===
        Reason: Cannot fetch non aggregated results with at least {} samples per group.
        Logical plan:
        {}",
                10, artifact.query_details,
            );
        
            loop {
                let mut ans = String::new();
                println!("Accept [y] or Reject [n]?");
                std::io::stdin()
                    .read_line(&mut ans)
                    .expect("Failed to read line");
        
                match ans.trim() {
                    "y" => break,
                    "n" => return Err(Status::invalid_argument(format!(
                        "The data owner rejected the fetch operation.
        Fetching a dataframe obtained with a non privacy-preserving query requires the approval of the data owner.
        This dataframe was obtained in a non privacy-preserving fashion as it does not aggregate results with at least {} samples per group.",
                        10
                    ))),
                    _ => continue,
                }
            }
        }
        Ok(artifact.dataframe.clone())
    }

    fn get_df_unchecked(&self, identifier: &str) -> Result<DataFrame, Status> {
        let dfs = self.dataframes.read().unwrap();
        Ok(dfs
            .get(identifier)
            .ok_or(Status::not_found(format!(
                "Could not find dataframe: identifier={}",
                identifier
            )))?
            .dataframe
            .clone())
    }

    fn get_header(&self, identifier: &str) -> Result<String, Status> {
        Ok(get_df_header(
            &self
                .dataframes
                .read()
                .unwrap()
                .get(identifier)
                .ok_or(Status::not_found(format!(
                    "Could not find dataframe: identifier={}",
                    identifier
                )))?
                .dataframe,
        )?)
    }

    fn get_headers(&self) -> Result<Vec<(String, String)>, Status> {
        let dataframes = self.dataframes.read().unwrap();
        let mut res = Vec::with_capacity(dataframes.len());
        for (k, v) in dataframes.iter() {
            let header = get_df_header(&v.dataframe)?;
            res.push((k.clone(), header));
        }
        Ok(res)
    }

    fn insert_df(&self, df: DataFrameArtifact) -> String {
        let mut dfs = self.dataframes.write().unwrap();
        let identifier = format!("{}", Uuid::new_v4());
        dfs.insert(identifier.clone(), df);
        identifier
    }
}

fn get_df_header(df: &DataFrame) -> Result<String, Status> {
    serde_json::to_string(&df.schema())
        .map_err(|e| Status::internal(format!("Could not serialize data frame header: {}", e)))
}

#[tonic::async_trait]
impl BastionLab for BastionLabState {
    type FetchDataFrameStream = ReceiverStream<Result<Chunk, Status>>;

    async fn run_query(
        &self,
        request: Request<Query>,
    ) -> Result<Response<ReferenceResponse>, Status> {
        let composite_plan: CompositePlan = serde_json::from_str(&request.get_ref().composite_plan)
            .map_err(|e| {
                Status::invalid_argument(format!(
                    "Could not deserialize composite plan: {}{}",
                    e,
                    &request.get_ref().composite_plan
                ))
            })?;
        let res = composite_plan.run(self)?;

        let header = get_df_header(&res.dataframe)?;
        let identifier = self.insert_df(res);
        Ok(Response::new(ReferenceResponse { identifier, header }))
    }

    async fn send_data_frame(
        &self,
        request: Request<Streaming<Chunk>>,
    ) -> Result<Response<ReferenceResponse>, Status> {
        let df = df_from_stream(request.into_inner()).await?;

        let header = get_df_header(&df)?;
        let identifier = self.insert_df(DataFrameArtifact::new(df));
        Ok(Response::new(ReferenceResponse { identifier, header }))
    }

    async fn fetch_data_frame(
        &self,
        request: Request<ReferenceRequest>,
    ) -> Result<Response<Self::FetchDataFrameStream>, Status> {
        let df = self.get_df(&request.get_ref().identifier)?;

        Ok(stream_data(df, 32).await)
    }

    async fn list_data_frames(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<ReferenceList>, Status> {
        let list = self
            .get_headers()?
            .into_iter()
            .map(|(identifier, header)| ReferenceResponse { identifier, header })
            .collect();

        Ok(Response::new(ReferenceList { list }))
    }

    async fn get_data_frame_header(
        &self,
        request: Request<ReferenceRequest>,
    ) -> Result<Response<ReferenceResponse>, Status> {
        let identifier = String::from(&request.get_ref().identifier);
        let header = self.get_header(&identifier)?;

        Ok(Response::new(ReferenceResponse { identifier, header }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let run_attestation = std::env::var("ATTESTATION").ok().as_deref() == Some("true");
    let state = BastionLabState::new();
    let attestation = if run_attestation {
        Some(BastionLabServer::new(BastionLabState::new()))
    }
    else { None };
    let addr = "0.0.0.0:50056".parse()?;
    println!("BastionLab server running...");
    Server::builder()
        .add_optional_service(attestation)
        .add_service(BastionLabServer::new(state))
        .serve(addr)
        .await?;
    Ok(())
}
