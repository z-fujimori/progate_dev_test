use actix_web::HttpResponse;
use chrono_tz::Asia::Srednekolymsk;
use std;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use actix_web::{get, post, web, Result, Responder};
use diesel::prelude::*;
use crate::api::uservoiceapi::get_user_qiita;
use crate::models::voicemodel::*;
use crate::schema::voices::{qiita_id};
use crate::schema::voices::dsl::*;
use diesel::Connection;
use diesel::pg::PgConnection;
use dotenv::dotenv;
use serde::Deserialize;
use reqwest::Client;
use serde_json::Value;
use tokio::time::{Duration, Instant};
use crate::models::uservoicemodel::*;
use crate::models::uservoicemodel::CreateUserVoice;
use crate::schema::user_voices::{qiita_id as uv_qiita_id, user_id};
use crate::schema::user_voices::dsl::*;

#[derive(Deserialize)]
struct Info {
    qiita_id: String,
    user_id: String,
    sprit_len: i32,
}
fn db_connect() -> PgConnection {
    dotenv().ok();
    let db_url = std::env::var("DATABASE_URL").expect("Database Must Be Set");
    PgConnection::establish(&db_url).expect(&format!("Error connecting to {}", &db_url))
}
pub struct ReturnCheckBody {
    body_text: String,
    body_code: Vec<String>
}
fn check_body(body_data: &str) -> ReturnCheckBody {
    let mut return_data = ReturnCheckBody {
        body_text: String::from(""),
        body_code: vec![]
    };
    let mut back_quotation_count = 0;
    let mut code_zone = false;
    let mut code_zone_start_n = false;
    let mut keep_code: Vec<char> = Vec::new();
    let mut befor_word = 'a';
    for ch in body_data.chars(){
        if befor_word != '`'{
            back_quotation_count = 0;
        }
        befor_word = ch;
        if ch == '`' { 
            back_quotation_count += 1;
            if back_quotation_count >= 3 {
                code_zone = !code_zone;
                back_quotation_count = 0;
                if code_zone {
                    code_zone_start_n = true;
                    return_data.body_text.pop();
                    return_data.body_text.pop();
                    continue;
                } else {
                    keep_code.pop();
                    keep_code.pop();
                    return_data.body_code.push(keep_code.iter().collect());
                    keep_code = Vec::new();
                    continue;
                }
            }
        }
        // '''で囲まれている箇所をコードとして保存
        if code_zone {
            if code_zone_start_n {
                if ch == '\n' {
                    code_zone_start_n = false;
                    continue;
                } else {
                    continue;
                }
            }
            keep_code.push(ch);
            continue;
        }
        return_data.body_text.push(ch);
    }
    return_data
}
fn split_text_length(text: &String, length: usize) -> Vec<String> {
    text.chars()
        .collect::<Vec<char>>() 
        .chunks(length)
        .map(|chunk| chunk.iter().collect())
        .collect()
}



#[post("/qiita")]
async fn save_qiita(info: web::Json<Info>) -> HttpResponse {

    // 既に保存してあるqiita_idかを確認したい、、、
    let binary_data = confilm_binary(info.qiita_id.clone()).await;

    if !binary_data.is_empty() {
        let first_record = binary_data.first().unwrap();
        return HttpResponse::Ok().content_type("audio/wav").body(first_record.voice_data.clone())
    }
    else {
        let client = Client::new(); // 1
        let url = format!("https://qiita.com/api/v2/items/{}",info.qiita_id);
        println!("{}",url);
        let response = client
            .get(url)
            .send()
            .await // 2
            .expect("Failed to send request");
        println!("response \n {:?}",response);
        // let body = response.text().await; // 3
        let body = response
            .text()
            .await
            .expect("Failed to read response text"); // ここでResultをアンラップ

        let mut return_data : String = "no data".to_string();

        let mut response_data: ReturnCheckBody;

        let json_str = body.clone();
        // JSON文字列をValueに変換
        let v: Value = serde_json::from_str(&json_str).unwrap();
        // rendered_bodyをデコードして表示
        if let Some(rendered_body) = v.get("body") {    // マークダウンを取得
            let decoded_html = rendered_body.as_str().unwrap();
            println!("{}", decoded_html);
            return_data = decoded_html.to_string();
            let list_string = return_data.as_str();
            response_data = check_body(list_string);
            println!("{:?}",response_data.body_text);
            println!("{:?}",response_data.body_code);
        } else {
            println!("No rendered_body found");
            let void_return = ReturnCheckBody {
                body_text: String::from(""),
                body_code: vec![]
            };
            response_data = void_return;
        }

        let audio_text = response_data.body_text.replace("\r\n", "").replace("```", "").replace("\n", "").replace("#", "");

        // テキストを100文字ずつに分割
        let chunks = split_text_length(&audio_text , 50);

        // wavファイルのバイナリデータを格納するためのバッファ
        let mut audio_binary: Vec<u8> = Vec::new();
        let mut header: Vec<u8> = Vec::with_capacity(44);
        let mut first_iteration = true;

        for chunk in chunks {
            let url = format!("http://voicevox:50021/audio_query?text={chunk}&speaker=3");
            // let url = format!("https://vvtk3mgv4r.us-west-2.awsapprunner.com/audio_query?text={chunk}&speaker=3");
            let response = client
                .post(url)
                .send()
                .await
                .expect("Failed to send voicevox audio_query request");

            let synthesis_response = client
                .post("http://voicevox:50021/synthesis?speaker=3")
                // .post("https://vvtk3mgv4r.us-west-2.awsapprunner.com/synthesis?speaker=3")
                .header("Content-Type", "application/json")
                .header("Accept", "audio/wav")
                .body(response)
                .send()
                .await
                .expect("Failed to send request");

            let audio_data = synthesis_response.bytes().await.unwrap();

            if first_iteration {
                // 最初のデータでヘッダーを取得し、オーディオデータのヘッダー部分をスキップ
                header.extend_from_slice(&audio_data.to_vec()[0..44]);
                audio_binary.extend_from_slice(&audio_data.to_vec()[44..]);
                first_iteration = false;
            } else {
                // 2回目以降はオーディオデータのみを追加
                audio_binary.extend_from_slice(&audio_data.to_vec()[44..]);
            }
        }

        // ファイルサイズとデータサイズを更新
        let data_size = audio_binary.len() as u32;
        let file_size = 36 + data_size; 

        header[4..8].copy_from_slice(&file_size.to_le_bytes());
        header[40..44].copy_from_slice(&data_size.to_le_bytes());

        let mut buffer: Vec<u8> = Vec::new();
        buffer.extend_from_slice(&header);
        buffer.extend_from_slice(&audio_binary);


        let input_voice = CreateVoice {
            voice_data: buffer.clone(), 
            qiita_id: String::from(&info.qiita_id), 
            title: String::from("title")
        };
        let mut connection = db_connect();
        let _return_voice_data = diesel::insert_into(voices)
            .values(&input_voice)
            .get_result::<VoiceResponse>(& mut connection)
            // .execute(& mut connection)
            .expect("Error inserting new time");

        // HTTPレスポンスを返す
        HttpResponse::Ok().content_type("audio/wav").body(buffer) // ここreturn_voice_data.~~~の形で書きたい
    }

    
}

#[get("/testbinary")]
async fn testbinary() -> HttpResponse {
    let mut connection = db_connect();
    let data:Vec<VoiceResponse> = voices.load(& mut connection).unwrap();
    let first_record = data.first().unwrap();

    HttpResponse::Ok().content_type("audio/wav").body(first_record.voice_data.clone())
}

#[post("/qiita/tokio")]
async fn save_qiita_tokio(info: web::Json<Info>) -> impl Responder {
    let binary_data = confilm_binary(info.qiita_id.clone()).await;

    if binary_data.len() > 0 {
        let confilm_user_voice = store_user_voices(info.user_id.clone(), info.qiita_id.clone()).await;
        println!("{:?}", confilm_user_voice);
        let first_record = binary_data.first().unwrap();
        return HttpResponse::Ok().content_type("audio/wav").body(first_record.voice_data.clone())
    }
    else {
        match process_qiita_tokio(info).await {
            Ok(response) => response,
            Err(e) => {
                eprintln!("Error: {:?}", e);
                HttpResponse::InternalServerError().finish()
            }
        }
    }
}

async fn confilm_binary(confilm_qiita_id:String) -> Vec<VoiceResponse> {
    let mut connection = db_connect();
    let data:Vec<VoiceResponse> = voices
    .filter(qiita_id.eq(confilm_qiita_id))
    .load(& mut connection)
    .unwrap();

    return data;
}

async fn store_user_voices(confilm_user_id:String, confilm_qiita_id:String) -> bool {
    let mut connection = db_connect();
    let data:Vec<UserVoiceResponse> = user_voices
        .filter(uv_qiita_id.eq(confilm_qiita_id.clone()))
        .filter(user_id.eq(confilm_user_id.clone()))
        .load(& mut connection)
        .unwrap();

    if !data.is_empty() {
        println!("{:?}",data);
        true
    }
    else {
        let input_user_voice = CreateUserVoice {
            user_id: String::from(confilm_user_id.clone()), 
            qiita_id: String::from(confilm_qiita_id.clone()), 
            // title: String::from(v_title.as_str().unwrap())
            title: String::from("title")
        };
        let return_user_voice_data = diesel::insert_into(user_voices)
            .values(&input_user_voice)
            .get_result::<UserVoiceResponse>(& mut connection)
            .expect("Error inserting new time");
        println!("{:?}",return_user_voice_data.qiita_id);
        false
    }
}

async fn process_qiita_tokio(info: web::Json<Info>) -> Result<HttpResponse, Box<dyn std::error::Error>> {
    let client = Client::new();
    let url = format!("https://qiita.com/api/v2/items/{}", info.qiita_id);
    println!("{}", url);

    // 時間計測始点
    let stert_time = Instant::now();
    
    let response = client.get(&url).send().await?;
    // println!("response \n {:?}", response);
    
    let body = response.text().await?;

    let v: Value = serde_json::from_str(&body)?;
    let v_title = v.get("title").unwrap();

    let end_qiita_time = Instant::now();
    println!("qiitaAPIデータ取得 : {:?}",end_qiita_time.checked_duration_since(stert_time));
    
    let response_data = if let Some(rendered_body) = v.get("body") {
        let decoded_html = rendered_body.as_str().unwrap();
        // println!("{}", decoded_html);
        let list_string = decoded_html;
        let response_data = check_body(list_string);
        // println!("{:?}", response_data.body_text);
        // println!("{:?}", response_data.body_code);
        response_data
    } else {
        println!("No rendered_body found");
        ReturnCheckBody {
            body_text: String::from(""),
            body_code: vec![]
        }
    };
    let audio_text = response_data.body_text.replace("\r\n", "").replace("```", "").replace("\n", "").replace("#", "");

    let split_length:usize = info.sprit_len as usize;

    let end_checktext_time = Instant::now();
    println!("整形終了 : {:?}",end_checktext_time.checked_duration_since(end_qiita_time));
    println!("文字数 : {}",&audio_text.len());
    println!("split length : {}",split_length);

    let chunks = split_text_length(&audio_text, split_length); //　文字分割
    let mut audio_binary: Vec<u8> = Vec::new();
    let mut header: Vec<u8> = Vec::with_capacity(44);
    // let mut first_iteration = true;
    println!("一個分の長さ : {}",chunks[0].len());
    println!("一個分のテキスト : {}",chunks[0]);
    
    let chunks_len = chunks.len();
    let audio_map = Arc::new(Mutex::new(HashMap::new()));

    let mut tasks = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let client = client.clone();
        let chunk = chunk.to_string();
        let audio_map = Arc::clone(&audio_map);

        let task = tokio::spawn(async move {
            let vv_number = i % 3;
            println!("Task : {}  vvNumber {}",i, vv_number);
            let url = format!("http://voicevox{vv_number}:50021/audio_query?text={chunk}&speaker=3");
            println!("{}",url);
            // let url = format!("https://vvtk3mgv4r.us-west-2.awsapprunner.com/audio_query?text={chunk}&speaker=3");
            let response = client.post(&url).send().await?;

            println!("Check : {}  vvNumber {}",i, vv_number);

            let url = format!("http://voicevox{vv_number}:50021/synthesis?speaker=3");

            let synthesis_response = client
                .post(&url)
                // .post("https://vvtk3mgv4r.us-west-2.awsapprunner.com/synthesis?speaker=3")
                .header("Content-Type", "application/json")
                .header("Accept", "audio/wav")
                .body(response.text().await?)
                .send()
                .await?;

            let audio_data = synthesis_response.bytes().await?;
            let mut audio_map = audio_map.lock().unwrap();
            audio_map.insert(i, audio_data);

            println!("End : {}  vvNumber {}",i, vv_number);

            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        });
        tasks.push(task);
    }

    for task in tasks {
        let _ = task.await?;
    }

    let end_vvapi_time = Instant::now();
    println!("音声変換終了 : {:?}",end_vvapi_time.checked_duration_since(end_checktext_time));

    let audio_map = Arc::try_unwrap(audio_map)
        .expect("Failed to unwrap Arc")
        .into_inner()
        .expect("Failed to get Mutex guard");

    for i in 0..chunks_len {
        let audio_data = audio_map.get(&i).unwrap();
        if i == 0 {
            header.extend_from_slice(&audio_data.to_vec()[0..44]);
            audio_binary.extend_from_slice(&audio_data.to_vec()[44..]);
            // first_iteration = false;
        } else {
            audio_binary.extend_from_slice(&audio_data.to_vec()[44..]);
        }
    }

    let data_size = audio_binary.len() as u32;
    let file_size = 36 + data_size; 

    header[4..8].copy_from_slice(&file_size.to_le_bytes());
    header[40..44].copy_from_slice(&data_size.to_le_bytes());

    let mut buffer: Vec<u8> = Vec::new();
    buffer.extend_from_slice(&header);
    buffer.extend_from_slice(&audio_binary);

    println!("{:?}",buffer);

    let input_voice = CreateVoice {
        voice_data: buffer.clone(), 
        qiita_id: String::from(&info.qiita_id), 
        title: String::from(v_title.as_str().unwrap())
    };

    // let mut connection = db_connect();
    // let return_voice_data = diesel::insert_into(voices)
    //     .values(&input_voice)
    //     .get_result::<VoiceResponse>(&mut connection)?;
    // let store_user_voice = store_user_voices(info.user_id.clone(), info.qiita_id.clone()).await;
    let end_this_api = Instant::now();
    println!("全体終了 : {:?}",end_this_api.checked_duration_since(end_vvapi_time));

    Ok(HttpResponse::Ok().content_type("audio/wav").body(buffer))
}
