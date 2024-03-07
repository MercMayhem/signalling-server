use std::env;

use lambda_http::{run, service_fn, tracing, Body, Error, Request, RequestExt, Response, request::RequestContext};
use aws_sdk_dynamodb::{Client, types::AttributeValue};
use aws_config::from_env;

/// This is the main body for the function.
/// Write your code inside it.
/// There are some code example in the following URLs:
/// - https://github.com/awslabs/aws-lambda-rust-runtime/tree/main/examples
async fn function_handler(event: Request) -> Result<Response<Body>, Error> {
    // Extract some useful information from the request
    
    let config = from_env().region("ap-south-1").load().await;
    let client = Client::new(&config);

    println!("The Config: {:?}", config);

    if let RequestContext::WebSocket(request_context) = event.request_context(){
        let route = request_context.route_key;
        let connection_id = request_context.connection_id;
        
        match route {
            Some(route) => {
                if route == "$connect" {
                    handle_connect(&client, &connection_id.unwrap()).await?;
                }
            },

            None => todo!()
        }

    } else {
        println!("request type: {:?}", event.request_context());
        return Err("Incorrect request type".to_string().into())
    }

    return Ok(Response::new(Body::Text("Working".to_string())));
}

async fn handle_connect(client: &Client, connection_id: &str) -> Result<(), Error>{
    let table = env::var("CONN_TABLE")?;
    let pkey = env::var("CONN_PKEY")?;

    client.put_item()
        .table_name(table)
        .item(pkey, AttributeValue::S(connection_id.to_string()))
        .send().await?;

    return Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing::init_default_subscriber();

    run(service_fn(function_handler)).await
}
