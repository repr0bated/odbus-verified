//! Direct provider-only text completion on an existing approved native route.
//! This code never opens the upstream agent websocket, exposes tools, interprets
//! tool calls, spawns jobs, delegates, reflects, or loops on model instructions.
use anyhow::{Result,Context,ensure,anyhow};
use futures::StreamExt;
use serde::{Serialize,Deserialize};
use serde_json::Value;
use op_agents::{inference::{CompleteTextInput,CompleteTextOutput,CompletionStatus},judgment::Usage};
use op_plugins::state_plugins::model_registry::ProviderAuth;
use super::{ChatRouterRuntimeClient,ConfiguredProvider,ConfigSecretPolicy,SYSTEM_ONE_HTTP};

#[derive(Debug,thiserror::Error)]
#[error("provider inference outcome or response disclosure is unresolved; preserve original operation, never replay")]
pub(crate) struct InferenceOutcomeUnknown;
pub(super) fn rejected_status(status:reqwest::StatusCode)->anyhow::Error {
    let category=match status.as_u16(){401|403=>"authentication_rejected",429=>"capacity_rejected",500..=599=>"provider_unavailable",400..=499=>"request_rejected",_=>"protocol_rejected"};
    anyhow::Error::new(InferenceOutcomeUnknown).context(format!("provider HTTP {category} ({})",status.as_u16()))
}
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct DirectRoute {pub selection:ConfiguredProvider,pub uri:String,pub wire_api:Option<String>}
impl DirectRoute {
    pub(super) fn approval_profile(&self)->op_agents::inference::NativeProviderProfile {
        op_agents::inference::NativeProviderProfile{provider_reference:self.selection.reference.clone(),model:self.selection.selected_model.clone(),
            http_uri:self.uri.clone(),wire_api:self.wire_api.clone()}
    }
}

impl ChatRouterRuntimeClient {
    pub(super) async fn direct_route(&self,secrets:&dyn ConfigSecretPolicy)->Result<DirectRoute>{
        let selection=self.current_provider().await?;
        let config=self.read_masked_config(secrets).await?;
        let agent=config.get("agents").and_then(|agents|agents.get(&self.agent_alias)).context("approved native agent alias is absent")?;
        ensure!(agent.get("enabled").and_then(Value::as_bool)==Some(true),"approved native agent is not enabled");
        ensure!(agent.get("model_provider").and_then(Value::as_str)==Some(selection.reference.as_str()),"native agent route changed");
        let(family,alias)=selection.reference.split_once('.').context("native provider reference invalid")?;
        let profile=config.get("providers").and_then(|x|x.get("models")).and_then(|x|x.get(family)).and_then(|x|x.get(alias)).context("native provider profile is absent")?;
        ensure!(profile.get("model").and_then(Value::as_str)==Some(selection.selected_model.as_str()),"native selected model changed");
        let uri=profile.get("uri").and_then(Value::as_str).context("native profile has no explicit HTTP inference URI")?.to_owned();
        // Reuse the canonical credential-free HTTPS parser/namespace evaluator.
        ensure!(crate::chat_router_catalog_binding::endpoint_matches(&uri,&uri)?,"native HTTP profile URI invalid");
        let wire_api=profile.get("wire_api").and_then(Value::as_str).map(str::to_owned);
        Ok(DirectRoute{selection,uri,wire_api})
    }

    pub(crate) async fn complete_text_under_guard(&self,input:CompleteTextInput,
        admission:std::sync::Arc<dyn crate::chat_router_generation::NativeProviderAdmission>,
        secrets:&dyn ConfigSecretPolicy)->Result<CompleteTextOutput>{
        input.validate()?;admission.check().await?;
        let route=self.direct_route(secrets).await?;
        admission.check_profile(&route.approval_profile()).await?;
        let suffix=match route.wire_api.as_deref(){Some("chat_completions")=>"chat/completions",Some("responses")=>"responses",_=>anyhow::bail!("native profile has no supported explicit direct text wire protocol")};
        let endpoint=format!("{}/{}",route.uri.trim_end_matches('/'),suffix);
        let(auth,key)=bound_credentials(&route,&endpoint).await?;
        let client=SYSTEM_ONE_HTTP.as_ref().map_err(|_|anyhow!("direct provider HTTPS client unavailable"))?;
        let mut request=client.post(&endpoint).header(reqwest::header::ACCEPT,"application/json");
        request=match(auth,key){(ProviderAuth::Bearer,Some(key))=>request.bearer_auth(key),(ProviderAuth::None,None)=>request,_=>anyhow::bail!("direct text protocol authentication is unsupported")};
        match route.wire_api.as_deref(){
            Some("chat_completions")=>{
                #[derive(Serialize)]
                struct Body<'a>{model:&'a str,messages:&'a [op_agents::inference::TextMessage],max_tokens:u32,stream:bool}
                request=request.json(&Body{model:&route.selection.selected_model,messages:&input.messages,max_tokens:input.max_output_tokens,stream:false});
            }
            Some("responses")=>{
                #[derive(Serialize)]
                struct Body<'a>{model:&'a str,input:&'a [op_agents::inference::TextMessage],max_output_tokens:u32,store:bool}
                request=request.json(&Body{model:&route.selection.selected_model,input:&input.messages,max_output_tokens:input.max_output_tokens,store:false});
            }
            _=>unreachable!("wire protocol was validated before credential access"),
        }
        admission.check().await?;
        // The sole effect: one bounded HTTP provider request, with no tool fields.
        let response=request.send().await.map_err(|_|InferenceOutcomeUnknown)?;
        if !response.status().is_success(){return Err(rejected_status(response.status()));}
        let mut bytes=Vec::new();let mut chunks=response.bytes_stream();
        while let Some(chunk)=chunks.next().await{
            let chunk=chunk.map_err(|_|InferenceOutcomeUnknown)?;
            ensure!(bytes.len().checked_add(chunk.len()).is_some_and(|size|size<=1024*1024),InferenceOutcomeUnknown);
            bytes.extend_from_slice(&chunk);
        }
        let mut output=match route.wire_api.as_deref(){
            Some("chat_completions")=>parse_chat(&bytes),Some("responses")=>parse_responses(&bytes),_=>unreachable!(),
        }.map_err(|_|InferenceOutcomeUnknown)?;
        ensure!(!output.content.trim().is_empty()&&output.content.len()<=256*1024
            && output.usage.output_tokens<=u64::from(input.max_output_tokens),InferenceOutcomeUnknown);
        admission.check().await.map_err(|_|InferenceOutcomeUnknown)?;
        ensure!(self.direct_route(secrets).await.map_err(|_|InferenceOutcomeUnknown)?==route,InferenceOutcomeUnknown);
        output.provider_reference=route.selection.reference;Ok(output)
    }
}

/// One existing protected provider owner supplies exact native-reference binding,
/// approved HTTP namespace, enabled state and credential. No family/model routing.
pub(super) async fn bound_credentials(route:&DirectRoute,target:&str)->Result<(ProviderAuth,Option<String>)>{
    let route=route.clone();let target=target.to_owned();
    tokio::task::spawn_blocking(move||{
        let env=crate::model_registry_dispatch::Environment::from_env();
        let config=crate::model_registry_dispatch::load_providers(&env)?;
        let mut matches=config.providers.iter().filter(|provider|provider.enabled&&provider.credential.as_deref()!=Some("off")
            &&(provider.native_provider_id.as_deref()==Some(route.selection.reference.as_str())
                ||provider.native_profile_overrides.get(&route.selection.selected_model)==Some(&route.selection.reference)));
        let provider=matches.next().context("native HTTP route lacks an approved provider binding")?;
        ensure!(matches.next().is_none(),"native HTTP route has ambiguous provider bindings");
        ensure!(provider.credential.as_deref()!=Some("oauth"),"native OAuth/CLI binding is not an approved direct HTTP credential");
        let prefix=provider.inference_url_prefix.as_deref().context("provider has no explicit HTTP inference namespace")?;
        ensure!(crate::chat_router_catalog_binding::endpoint_matches(&route.uri,prefix)?
            &&crate::chat_router_catalog_binding::endpoint_matches(&target,prefix)?,"native profile/request outside approved HTTP namespace");
        let key=match provider.auth{
            ProviderAuth::None=>None,
            ProviderAuth::Bearer=>Some(crate::model_registry_dispatch::read_provider_key(&env,provider.key_file.as_deref().context("provider key file is not configured")?)?),
            _=>anyhow::bail!("direct provider authentication protocol unsupported"),
        };
        Ok((provider.auth,key))
    }).await.context("provider credential owner unavailable")?
}

#[derive(Deserialize)]
struct ChatResponse {model:String,choices:Vec<ChatChoice>,usage:ChatUsage}
#[derive(Deserialize)]
struct ChatChoice {index:u32,message:AssistantMessage,finish_reason:String}
#[derive(Deserialize)]
struct AssistantMessage {role:String,content:Option<String>,tool_calls:Option<Value>,function_call:Option<Value>}
#[derive(Deserialize)]
struct ChatUsage {prompt_tokens:u64,completion_tokens:u64}
fn parse_chat(bytes:&[u8])->Result<CompleteTextOutput>{
    let response:ChatResponse=serde_json::from_slice(bytes)?;
    ensure!(response.choices.len()==1&&!response.model.is_empty(),"unexpected completion shape");
    let choice=response.choices.into_iter().next().context("completion choice absent")?;
    ensure!(choice.index==0&&choice.message.role=="assistant"&&choice.message.tool_calls.is_none()&&choice.message.function_call.is_none(),"non-text completion refused");
    let status=match choice.finish_reason.as_str(){"stop"=>CompletionStatus::Complete,"length"=>CompletionStatus::Truncated,_=>anyhow::bail!("completion did not finish as text")};
    Ok(CompleteTextOutput{content:choice.message.content.context("text completion absent")?,model:response.model,provider_reference:String::new(),status,
        usage:Usage{input_tokens:response.usage.prompt_tokens,output_tokens:response.usage.completion_tokens}})
}
#[derive(Deserialize)]
struct ResponsesResponse {model:String,status:String,output:Vec<ResponseItem>,usage:ResponsesUsage}
#[derive(Deserialize)]
struct ResponsesUsage {input_tokens:u64,output_tokens:u64}
#[derive(Deserialize)]
struct ResponseItem {#[serde(rename="type")]kind:String,role:Option<String>,content:Option<Vec<ResponseText>>}
#[derive(Deserialize)]
struct ResponseText {#[serde(rename="type")]kind:String,text:Option<String>}
fn parse_responses(bytes:&[u8])->Result<CompleteTextOutput>{
    let response:ResponsesResponse=serde_json::from_slice(bytes)?;ensure!(!response.model.is_empty(),"response model absent");
    let status=match response.status.as_str(){"completed"=>CompletionStatus::Complete,"incomplete"=>CompletionStatus::Truncated,_=>anyhow::bail!("response not complete text")};
    let mut content=String::new();for item in response.output{
        match item.kind.as_str(){
            "reasoning"=>{},
            "message"=>{ensure!(item.role.as_deref()==Some("assistant"),"response role invalid");for part in item.content.context("response text missing")?{
                ensure!(part.kind=="output_text","non-text response content refused");let text=part.text.context("response text absent")?;
                ensure!(content.len().checked_add(text.len()).is_some_and(|size|size<=256*1024),"response text bound exceeded");content.push_str(&text);
            }},
            _=>anyhow::bail!("tool/function/delegate response refused without execution"),
        }
    }
    Ok(CompleteTextOutput{content,model:response.model,provider_reference:String::new(),status,
        usage:Usage{input_tokens:response.usage.input_tokens,output_tokens:response.usage.output_tokens}})
}
