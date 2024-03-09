use std::env;
use uuid::Uuid;
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

    if let RequestContext::WebSocket(request_context) = event.request_context(){
        let route = request_context.route_key;
        let connection_id = request_context.connection_id.unwrap();
        
        match route {
            Some(route) => {
                if route == "$connect" {
                    handle_connect(&client, &connection_id).await?;
                }

                if route == "create" {
                    handle_create(&client, &connection_id).await?;
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

async fn handle_create(client: &Client, connection_id: &str) -> Result<(), Error>{
    let connection_table = env::var("CONN_TABLE")?;
    let room_table = env::var("ROOM_TABLE")?;
    let connection_pkey = env::var("CONN_PKEY")?;
    let room_pkey = env::var("ROOM_PKEY")?;

    let uuid = Uuid::new_v4();

    client.update_item()
        .table_name(connection_table)
        .key(connection_pkey, AttributeValue::S(connection_id.to_string()))
        .update_expression("SET RoomID = :uuid")
        .expression_attribute_values(":uuid", AttributeValue::S(uuid.to_string()))
        .send().await?;
    
    client.put_item()
        .table_name(room_table)
        .item(room_pkey, AttributeValue::S(uuid.to_string()))
        .item("created_by", AttributeValue::S(connection_id.to_string()))
        .item("subscribers", AttributeValue::Ss(vec![connection_id.to_string()]))
        .send().await?;

    return Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing::init_default_subscriber();

    run(service_fn(function_handler)).await
}
