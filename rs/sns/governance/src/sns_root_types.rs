// NOTE: This file's types are all from other canisters where a current dependency cycle prevents
// including them directly.
// TODO(NNS1-1589): Remove all these types after dependency cycle is fixed.

#[derive(
    candid::CandidType,
    candid::Deserialize,
    comparable::Comparable,
    Clone,
    PartialEq,
    ::prost::Message,
)]
pub struct RegisterDappCanisterRequest {
    #[prost(message, optional, tag = "1")]
    pub canister_id: ::core::option::Option<::ic_base_types::PrincipalId>,
}
#[derive(
    candid::CandidType,
    candid::Deserialize,
    comparable::Comparable,
    Clone,
    PartialEq,
    ::prost::Message,
)]
pub struct RegisterDappCanisterResponse {}
/// This message has an identical message defined in governace.proto, both need to be changed together
/// TODO(NNS1-1589)
#[derive(
    candid::CandidType,
    candid::Deserialize,
    comparable::Comparable,
    Clone,
    PartialEq,
    ::prost::Message,
)]
pub struct RegisterDappCanistersRequest {
    #[prost(message, repeated, tag = "1")]
    pub canister_ids: ::prost::alloc::vec::Vec<::ic_base_types::PrincipalId>,
}
#[derive(
    candid::CandidType,
    candid::Deserialize,
    comparable::Comparable,
    Clone,
    PartialEq,
    ::prost::Message,
)]
pub struct RegisterDappCanistersResponse {}
/// Change control of the listed canisters to the listed principal id.
/// Same proto in governance.proto. TODO(NNS1-1589)
#[derive(
    candid::CandidType,
    candid::Deserialize,
    comparable::Comparable,
    Clone,
    PartialEq,
    ::prost::Message,
)]
pub struct SetDappControllersRequest {
    #[prost(message, optional, tag = "1")]
    pub canister_ids: ::core::option::Option<set_dapp_controllers_request::CanisterIds>,
    #[prost(message, repeated, tag = "2")]
    pub controller_principal_ids: ::prost::alloc::vec::Vec<::ic_base_types::PrincipalId>,
}
/// Nested message and enum types in `SetDappControllersRequest`.
pub mod set_dapp_controllers_request {
    #[derive(
        candid::CandidType,
        candid::Deserialize,
        comparable::Comparable,
        Clone,
        PartialEq,
        ::prost::Message,
    )]
    pub struct CanisterIds {
        #[prost(message, repeated, tag = "1")]
        pub canister_ids: ::prost::alloc::vec::Vec<::ic_base_types::PrincipalId>,
    }
}
#[derive(
    candid::CandidType,
    candid::Deserialize,
    comparable::Comparable,
    Clone,
    PartialEq,
    ::prost::Message,
)]
pub struct SetDappControllersResponse {
    #[prost(message, repeated, tag = "1")]
    pub failed_updates: ::prost::alloc::vec::Vec<set_dapp_controllers_response::FailedUpdate>,
}
/// Nested message and enum types in `SetDappControllersResponse`.
pub mod set_dapp_controllers_response {
    #[derive(
        candid::CandidType,
        candid::Deserialize,
        comparable::Comparable,
        Clone,
        PartialEq,
        ::prost::Message,
    )]
    pub struct FailedUpdate {
        #[prost(message, optional, tag = "1")]
        pub dapp_canister_id: ::core::option::Option<::ic_base_types::PrincipalId>,
        #[prost(message, optional, tag = "2")]
        pub err: ::core::option::Option<super::CanisterCallError>,
    }
}
#[derive(
    candid::CandidType,
    candid::Deserialize,
    comparable::Comparable,
    Clone,
    PartialEq,
    ::prost::Message,
)]
pub struct CanisterCallError {
    #[prost(int32, optional, tag = "1")]
    pub code: ::core::option::Option<i32>,
    #[prost(string, tag = "2")]
    pub description: ::prost::alloc::string::String,
}
