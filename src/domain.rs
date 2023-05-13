use serde::Deserialize;
use serde::Serialize;



#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnsNotification {
    #[serde(rename = "Type")]
    pub type_field: SnsNotificationType,
    #[serde(rename = "Message")]
    pub message: Option<String>,

    #[serde(rename = "SubscribeURL")]
    pub subscribe_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SnsNotificationType {
    SubscriptionConfirmation,
    Notification,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub notification_type: NotificationType,
    pub bounce: Bounce,
    pub mail: Mail,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NotificationType {
    Bounce,
    Complaint,
    Delivery,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bounce {
    pub feedback_id: String,
    pub bounce_type: String,
    pub bounce_sub_type: String,
    pub bounced_recipients: Vec<BouncedRecipient>,
    pub timestamp: String,
    pub remote_mta_ip: String,
    #[serde(rename = "reportingMTA")]
    pub reporting_mta: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BouncedRecipient {
    pub email_address: String,
    pub action: Option<String>,
    pub status: Option<String>,
    pub diagnostic_code: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mail {
    pub timestamp: String,
    pub source: String,
    pub source_arn: String,
    pub source_ip: String,
    pub caller_identity: String,
    pub sending_account_id: String,
    pub message_id: String,
    pub destination: Vec<String>,
}


#[derive( Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Blacklist {
    pub id: Option<u32>,
    pub domain_id: u32,
    pub email: String,
    pub reason: String,
}