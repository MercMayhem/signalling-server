use std::env;
use uuid::Uuid;
use serde_json;
use lambda_http::{run, service_fn, tracing, Body, Error, Request, RequestExt, Response, request::RequestContext};
use aws_sdk_dynamodb::{Client, types::{AttributeValue, Update, TransactWriteItem}};
use aws_config::from_env;
use serde::Deserialize;

/// This is the main body for the function.
/// Write your code inside it.
/// There are some code example in the following URLs:
/// - https://github.com/awslabs/aws-lambda-rust-runtime/tree/main/examples

struct Config{
    connection_table: String,
    room_table: String,
    connection_pkey: String,
    room_pkey: String
}

#[derive(Deserialize)]
struct SubscribeBody{
    room_id: String
}

async fn function_handler(event: Request) -> Result<Response<Body>, Error> {
    // Extract some useful information from the request
    
    let config = from_env().region("ap-south-1").load().await;
    let client = Client::new(&config);
    
    let connection_table = env::var("CONN_TABLE")?;
    let room_table = env::var("ROOM_TABLE")?;
    let connection_pkey = env::var("CONN_PKEY")?;
    let room_pkey = env::var("ROOM_PKEY")?;

    let env = Config { connection_table, room_table, connection_pkey, room_pkey };

    if let RequestContext::WebSocket(request_context) = event.request_context(){
        let route = request_context.route_key;
        let connection_id = request_context.connection_id.unwrap();
        
        match route {
            Some(route) => {
                if route == "$connect" {
                    handle_connect(&client, &connection_id, &env).await?;
                }

                else if route == "create" {
                    handle_create(&client, &connection_id, &env).await?;
                }

                else if route == "subscribe" {
                    let body = event.body();
                    handle_subscribe(&client, &connection_id, body, &env).await?;

                }

                else if route == "unsubscribe" {
                    handle_unsubscribe(&client, &connection_id, &env).await?;
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

async fn handle_connect(client: &Client, connection_id: &str, env: &Config) -> Result<(), Error>{
    client.put_item()
        .table_name(&env.connection_table)
        .item(&env.connection_pkey, AttributeValue::S(connection_id.to_string()))
        .send().await?;

    return Ok(())
}

async fn handle_create(client: &Client, connection_id: &str, env:&Config) -> Result<(), Error>{

    let uuid = Uuid::new_v4();

    // Need to make the following atomic

    client.update_item()
        .table_name(&env.connection_table)
        .key(&env.connection_pkey, AttributeValue::S(connection_id.to_string()))
        .update_expression("SET RoomID = :uuid")
        .expression_attribute_values(":uuid", AttributeValue::S(uuid.to_string()))
        .send().await?;
    
    client.put_item()
        .table_name(&env.room_table)
        .item(&env.room_pkey, AttributeValue::S(uuid.to_string()))
        .item("created_by", AttributeValue::S(connection_id.to_string()))
        .item("subscribers", AttributeValue::L(vec![AttributeValue::S(connection_id.to_string())]))
        .send().await?;

    Ok(())
}

async fn handle_subscribe(client: &Client, connection_id: &str, body: &Body, env: &Config) -> Result<(), Error>{
    let body_data: SubscribeBody;

    if let Body::Text(data) = body {
        body_data = serde_json::from_str::<SubscribeBody>(data)?;
    }

    else{
        return Err("ERROR: Body not of the correct type".into());
    }
    
    let room_id = body_data.room_id;
    
    // Need to make the following atomic

    client.update_item()
        .table_name(&env.room_table)
        .key(&env.room_pkey, AttributeValue::S(room_id.clone()))
        .update_expression("SET subscribers = list_append(subscribers, :connection)")
        .expression_attribute_values(":connection", AttributeValue::L(vec!(AttributeValue::S(connection_id.to_string()))))
        .send().await?;

    client.update_item()
        .table_name(&env.connection_table)
        .key(&env.connection_pkey, AttributeValue::S(connection_id.to_string()))
        .update_expression("SET RoomID = :room")
        .expression_attribute_values(":room", AttributeValue::S(room_id))
        .send().await?;

    Ok(())
}

async fn handle_unsubscribe(client: &Client, connection_id: &str, env: &Config) -> Result<(), Error> {
    // Get room in which connection is present
    let mut response = client.get_item()
                .table_name(&env.connection_table)
                .key(&env.connection_pkey, AttributeValue::S(connection_id.to_string()))
                .projection_expression("RoomID")
                .send().await?;

    let item = response.item.take();
    let mut room_id: String = "".to_string();

    if let Some(val) = item {
        room_id = val.get("RoomID")
                        .map(|attribute_val| if let AttributeValue::S(val) = attribute_val 
                             { return val.to_string() }
                             else { "".to_string() })
                        .unwrap();
    }

    else {
        return Err("didn't receive item".into())
    }

    if room_id == ""{
        return Err("connection not associated with a room".into());
    }
    
    // Get index of connectionID in rooms table
    response = client.get_item()
                .table_name(&env.room_table)
                .key(&env.room_pkey, AttributeValue::S(room_id.to_string()))
                .projection_expression("subscribers")
                .send().await?;

    let room_item = response.item.take();
    let connection_list_index = room_item
                                .filter(|item| item.get("subscribers").is_some() )
                                .filter(|item| if let AttributeValue::L(_) = item.get("subscribers").unwrap(){
                                    return true;
                                } else { return false})
                                .map(|item| {
                                    let list = item.get("subscribers").unwrap().as_l().unwrap();
                                    for (index, connection) in list.iter().enumerate(){
                                        if connection.as_s().is_ok() && connection.as_s().unwrap() == connection_id {return Some(index)}
                                    }
                                    return None
                                })
                                .unwrap();

    if connection_list_index.is_none() { return Err("Couldn't find the connection id in subscribers list".into()) }
    let connection_list_index = connection_list_index.unwrap();

    // Craft transaction in rooms
    let update_operation = Update::builder()
                            .table_name(&env.room_table)
                            .key(&env.room_pkey, AttributeValue::S(room_id.to_string()))
                            .update_expression(format!("REMOVE subscribers[{}]", connection_list_index.to_string()))
                            .build()?;

    let transaction_1 = TransactWriteItem::builder()
                            .set_update(Some(update_operation))
                            .build();
    
    // Craft transaction in connections
    let update_operation2 = Update::builder()
                            .table_name(&env.connection_table)
                            .key(&env.connection_pkey, AttributeValue::S(connection_id.to_string()))
                            .update_expression("REMOVE RoomID")
                            .build()?;

    let transaction_2 = TransactWriteItem::builder()
                            .set_update(Some(update_operation2))
                            .build();

    // Send Query transanction
    
    client.transact_write_items()
        .transact_items(transaction_1)
        .transact_items(transaction_2)
        .send().await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing::init_default_subscriber();

    run(service_fn(function_handler)).await
}
