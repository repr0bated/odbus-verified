//! Atomic Jev questions and total deterministic outcomes. No model selection or authorization.
use std::collections::{BTreeMap, BTreeSet};
use anyhow::{Result, ensure, Context};
use schemars::JsonSchema;
use serde::{Serialize, Deserialize};
use serde_json::{Value, json};
use crate::recipes::validate_contract;

pub const POLICY_VERSION: &str = "native-orchestration-fit-v1";
pub const YES_ABOVE: f64 = 0.7;
pub const NO_BELOW: f64 = 0.3;
pub const MAX_JUDGMENT_STATE_BYTES: usize = 64 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag="type",rename_all="snake_case")]
pub enum Question {
    Noul { instructions: String, criteria: BTreeMap<String,String> },
    Choice { instructions: String, criteria: BTreeMap<String,String> },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SystemOneInput {
    /// The configured approved JEV route owns the model; callers cannot select one.
    pub state: Value,
    pub questions: BTreeMap<String,Question>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag="type",rename_all="snake_case")]
pub enum Answer {
    Noul { noul: f64 },
    Choice { choice: String, probabilities: BTreeMap<String,f64>, confidence: Option<f64> },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Usage { pub input_tokens:u64, pub output_tokens:u64 }
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SystemOneOutput { pub model:String, pub answers:BTreeMap<String,Answer>, pub usage:Usage }
impl SystemOneInput {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.questions.is_empty() && self.questions.len()<=128, "judgment question capacity exceeded");
        ensure!(serde_json::to_vec(self)?.len()<=MAX_JUDGMENT_STATE_BYTES, "judgment state exceeds approved bound");
        for (id,question) in &self.questions {
            ensure!(!id.is_empty() && id.len()<=256 && !id.chars().any(char::is_control), "invalid judgment question ID");
            let (instructions,criteria)=match question { Question::Noul {instructions,criteria}|Question::Choice {instructions,criteria}=>(instructions,criteria) };
            ensure!(!instructions.is_empty() && instructions.len()<=4096 && !criteria.is_empty() && criteria.len()<=128, "invalid atomic judgment question");
            if matches!(question,Question::Noul {..}) { ensure!(criteria.len()==2 && criteria.contains_key("true") && criteria.contains_key("false"), "Noul needs concrete positive and null criteria"); }
        }
        Ok(())
    }
}
impl SystemOneOutput {
    pub fn validate_for(&self, request:&SystemOneInput)->Result<()> {
        request.validate()?;
        ensure!(!self.model.is_empty() && self.answers.len()==request.questions.len(), "judgment response missing model or answers");
        for (id,question) in &request.questions {
            match (question,self.answers.get(id).context("judgment answer missing")?) {
                (Question::Noul {..},Answer::Noul {noul})=>probability(*noul)?,
                (Question::Choice {criteria,..},Answer::Choice {choice,probabilities,confidence})=>{
                    ensure!(criteria.keys().collect::<BTreeSet<_>>()==probabilities.keys().collect(), "judgment returned out-of-set options");
                    ensure!(probabilities.contains_key(choice), "judgment choice outside candidates");
                    let mut sum=0.0; for value in probabilities.values(){probability(*value)?;sum+=value;}
                    ensure!((sum-1.0).abs()<=0.000_001, "judgment probabilities do not sum to one");
                    if let Some(value)=confidence {probability(*value)?;}
                }
                _=>anyhow::bail!("judgment answer type mismatch"),
            }
        }
        Ok(())
    }
}
fn probability(p:f64)->Result<()> {ensure!(p.is_finite() && (0.0..=1.0).contains(&p),"invalid judgment probability");Ok(())}
fn noul(instructions:&str,positive:&str,negative:&str)->Question {
    Question::Noul {instructions:instructions.into(),criteria:BTreeMap::from([("true".into(),positive.into()),("false".into(),negative.into())])}
}
#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CandidateDescription { pub id:String,pub description:String }
#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema)]
#[serde(tag="outcome",rename_all="snake_case")]
pub enum SelectionOutcome {
    Selected {candidate_id:String,probability:f64,coverage:f64},
    NoAuthorizedCandidate,
    NoSemanticMatch,
    NeedsReview,
    JudgmentUnavailable,
    AuthorizationChanged,
}

pub fn selection_request(task:&str,candidates:&[CandidateDescription],coverage:bool)->Result<SystemOneInput> {
    ensure!(!task.trim().is_empty() && task.len()<=16*1024 && !candidates.is_empty() && candidates.len()<=128,"invalid approved selection state");
    let criteria:BTreeMap<_,_>=candidates.iter().map(|c|(c.id.clone(),c.description.clone())).collect();
    ensure!(criteria.len()==candidates.len(),"duplicate semantic candidate");
    let mut questions=BTreeMap::from([
        ("candidate".into(),Question::Choice { instructions:"Which candidate's expertise best matches the requested analysis in `task`?".into(),criteria }),
        ("any_fit".into(),noul("Does any candidate describe expertise needed for the requested analysis in `task`?","At least one provided description covers the analysis needed by this task.","The expertise needed by the task is absent from all provided descriptions.")),
    ]);
    if coverage { for candidate in candidates {
        questions.insert(format!("coverage:{}",candidate.id),noul(
            &format!("Does the documented expertise of `{}` cover the requested analysis in `task`?",candidate.id),
            &candidate.description,"This candidate's documented expertise does not cover the requested analysis."));
    } }
    let request=SystemOneInput {state:json!({"task":task,"candidates":candidates}),questions};request.validate()?;Ok(request)
}

#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema)]
#[serde(tag="outcome",rename_all="snake_case")]
pub enum ShortlistOutcome {
    Candidates { ids:Vec<String> },
    NoSemanticMatch,
    NeedsReview,
    JudgmentUnavailable,
}
pub fn shortlist(request:&SystemOneInput,reply:&SystemOneOutput)->ShortlistOutcome {
    if reply.validate_for(request).is_err(){return ShortlistOutcome::JudgmentUnavailable;}
    let Some(Answer::Noul{noul:fit})=reply.answers.get("any_fit")else{return ShortlistOutcome::JudgmentUnavailable};
    if *fit<NO_BELOW{return ShortlistOutcome::NoSemanticMatch;}
    if *fit<=YES_ABOVE{return ShortlistOutcome::NeedsReview;}
    let Some(Answer::Choice{probabilities,..})=reply.answers.get("candidate")else{return ShortlistOutcome::JudgmentUnavailable};
    let mut ranked:Vec<_>=probabilities.iter().collect();
    ranked.sort_by(|a,b|b.1.total_cmp(a.1).then_with(||a.0.cmp(b.0)));
    ShortlistOutcome::Candidates {ids:ranked.into_iter().take(3).map(|(id,_)|id.clone()).collect()}
}
pub fn decide(request:&SystemOneInput,reply:Option<&SystemOneOutput>,require_coverage:bool)->SelectionOutcome {
    let Some(reply)=reply else{return SelectionOutcome::JudgmentUnavailable};
    if reply.validate_for(request).is_err(){return SelectionOutcome::JudgmentUnavailable;}
    let Some(Answer::Noul{noul:any_fit})=reply.answers.get("any_fit")else{return SelectionOutcome::JudgmentUnavailable};
    if *any_fit<NO_BELOW{return SelectionOutcome::NoSemanticMatch;}
    if *any_fit<=YES_ABOVE{return SelectionOutcome::NeedsReview;}
    let Some(Answer::Choice{choice,probabilities,..})=reply.answers.get("candidate")else{return SelectionOutcome::JudgmentUnavailable};
    let Some(&selected)=probabilities.get(choice)else{return SelectionOutcome::JudgmentUnavailable};
    if selected<=YES_ABOVE || probabilities.iter().any(|(id,p)|id!=choice && *p>=selected){return SelectionOutcome::NeedsReview;}
    let coverage=if require_coverage{
        let Some(Answer::Noul{noul})=reply.answers.get(&format!("coverage:{choice}"))else{return SelectionOutcome::JudgmentUnavailable};
        *noul
    }else{*any_fit};
    if coverage<=YES_ABOVE{return SelectionOutcome::NeedsReview;}
    SelectionOutcome::Selected{candidate_id:choice.clone(),probability:selected,coverage}
}

#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JudgmentEvidence {
    pub policy_version:String,
    pub request_digest:String,
    pub intent_digest:String,
    pub target_binding_digest:String,
    pub operation_id:String,
    pub terminal_event_id:u64,
    pub terminal_event_hash:String,
}
pub fn checkpoint_request(intent:&str,operation:&Value,result_evidence:Option<&Value>)->Result<SystemOneInput> {
    let post=result_evidence.is_some();
    let question=if post{noul("Does the supplied evidence support this claimed task result?","The provided attributed source and receipt evidence supports the stated result.","The claimed result is unsupported, contradicted or unrelated to the provided evidence.")}
        else{noul("Does this described operation address the stated task intent?","The discovered operation and supplied arguments address the stated task intent.","The operation or arguments are unrelated to or contradict the stated task intent.")};
    let request=SystemOneInput {state:json!({"task":intent,"operation":operation,"evidence":result_evidence}),questions:BTreeMap::from([("appropriate".into(),question)])};
    request.validate()?;Ok(request)
}
pub fn checkpoint_decision(request:&SystemOneInput,reply:Option<&SystemOneOutput>)->SelectionOutcome {
    let Some(reply)=reply else{return SelectionOutcome::JudgmentUnavailable};
    if reply.validate_for(request).is_err(){return SelectionOutcome::JudgmentUnavailable;}
    match reply.answers.get("appropriate"){
        Some(Answer::Noul{noul})if *noul>YES_ABOVE=>SelectionOutcome::Selected{candidate_id:"appropriate".into(),probability:*noul,coverage:*noul},
        Some(Answer::Noul{noul})if *noul<NO_BELOW=>SelectionOutcome::NoSemanticMatch,
        Some(Answer::Noul{..})=>SelectionOutcome::NeedsReview,
        _=>SelectionOutcome::JudgmentUnavailable,
    }
}

pub fn validate_intent(intent:&crate::recipes::IntentSpec,now:i64)->Result<()> {
    ensure!(!intent.goal.trim().is_empty() && intent.goal.len()<=16*1024 && !intent.success_evidence.is_empty(),"intent outcome/evidence unresolved");
    intent.budget.validate(now)?;validate_contract(&intent.desired_output_contract)?;
    for resource in &intent.resources{crate::recipes::validate_resource_selector(resource)?;}
    ensure!(intent.dependencies.len()<=64 && intent.dependencies.iter().collect::<BTreeSet<_>>().len()==intent.dependencies.len(),"invalid intent dependencies");
    Ok(())
}
