use crate::canister_http::lib::get_universal_vm_address;
use crate::driver::boundary_node::{BoundaryNode, BoundaryNodeVm};
use crate::driver::ic::{InternetComputer, NrOfVCPUs, Subnet, VmResources};
use crate::driver::test_env::TestEnv;
use crate::driver::test_env_api::{
    await_boundary_node_healthy, HasDependencies, HasPublicApiUrl, HasTopologySnapshot,
    IcNodeContainer, NnsCanisterWasmStrategy, NnsInstallationBuilder, SubnetSnapshot,
    TopologySnapshot,
};
use crate::driver::universal_vm::UniversalVm;
use ic_registry_routing_table::canister_id_into_u64;
use ic_registry_subnet_features::SubnetFeatures;
use ic_registry_subnet_type::SubnetType;
use ic_types::SubnetId;
use slog::{info, Logger};
use std::process::{Command, Stdio};

pub const UNIVERSAL_VM_NAME: &str = "httpbin";

const BOUNDARY_NODE_NAME: &str = "boundary-node-1";

const REPLICATION_FACTOR: usize = 2;

const EXCLUDED: &[&str] = &[
    // to start with something that is always false
    "(1 == 0)",
    // the replica does not yet check that the effective canister id is valid in all cases
    "$0 ~ /wrong effective canister id.in management call/",
    "$0 ~ /access denied with different effective canister id/",
    // Recursive calls from queries are now allowed.
    // When composite queries are enabled, we should clean up and re-enable this test
    "$0 ~ /Call from query method traps (in query call)/",
];

pub fn config_impl(env: TestEnv, deploy_bn_and_nns_canisters: bool, http_requests: bool) {
    use crate::driver::test_env_api::{retry, secs};
    use crate::util::block_on;
    use hyper::client::connect::HttpConnector;
    use hyper::Client;
    use hyper_tls::HttpsConnector;
    use std::env;

    let vm_resources = VmResources {
        vcpus: Some(NrOfVCPUs::new(16)),
        memory_kibibytes: None,
        boot_image_minimal_size_gibibytes: None,
    };
    InternetComputer::new()
        .add_subnet(
            Subnet::new(SubnetType::System)
                .with_default_vm_resources(vm_resources)
                .with_features(SubnetFeatures {
                    http_requests,
                    ..SubnetFeatures::default()
                })
                .add_nodes(REPLICATION_FACTOR),
        )
        .add_subnet(
            Subnet::new(SubnetType::Application)
                .with_default_vm_resources(vm_resources)
                .with_features(SubnetFeatures {
                    http_requests,
                    ..SubnetFeatures::default()
                })
                .add_nodes(REPLICATION_FACTOR),
        )
        .setup_and_start(&env)
        .expect("failed to setup IC under test");
    if deploy_bn_and_nns_canisters {
        let nns_node = env
            .topology_snapshot()
            .root_subnet()
            .nodes()
            .next()
            .unwrap();
        NnsInstallationBuilder::new()
            .with_canister_wasm_strategy(NnsCanisterWasmStrategy::TakeBuiltFromSources)
            .install(&nns_node, &env)
            .expect("NNS canisters not installed");
        info!(env.logger(), "NNS canisters are installed.");
        BoundaryNode::new(String::from(BOUNDARY_NODE_NAME))
            .allocate_vm(&env)
            .expect("Allocation of BoundaryNode failed.")
            .for_ic(&env, "")
            .use_ipv6_certs()
            .start(&env)
            .expect("failed to setup BoundaryNode VM");
    }
    env.topology_snapshot().subnets().for_each(|subnet| {
        subnet
            .nodes()
            .for_each(|node| node.await_status_is_healthy().unwrap())
    });

    if http_requests {
        env::set_var(
            "SSL_CERT_FILE",
            env.get_dependency_path("ic-os/guestos/rootfs/dev-certs/canister_http_test_ca.cert"),
        );
        env::remove_var("NIX_SSL_CERT_FILE");

        // Set up Universal VM for httpbin testing service
        UniversalVm::new(String::from(UNIVERSAL_VM_NAME))
            .with_config_img(env.get_dependency_path("rs/tests/http_uvm_config_image.zst"))
            .start(&env)
            .expect("failed to set up universal VM");
        let log = env.logger();
        retry(log.clone(), secs(300), secs(10), || {
            block_on(async {
                let mut http_connector = HttpConnector::new();
                http_connector.enforce_http(false);
                let mut https_connector = HttpsConnector::new_with_connector(http_connector);
                https_connector.https_only(true);
                let client = Client::builder().build::<_, hyper::Body>(https_connector);

                let webserver_ipv6 = get_universal_vm_address(&env);
                let httpbin = format!("https://[{webserver_ipv6}]:20443");
                let req = hyper::Request::builder()
                    .method(hyper::Method::GET)
                    .uri(httpbin)
                    .body(hyper::Body::from(""))?;

                let resp = client.request(req).await?;

                let body_bytes = hyper::body::to_bytes(resp.into_body()).await?;
                let body = String::from_utf8(body_bytes.to_vec()).unwrap();

                info!(log, "response body from httpbin: {}", body);

                Ok(())
            })
        })
        .expect("Httpbin server should respond to incoming requests!");
    }
    if deploy_bn_and_nns_canisters {
        await_boundary_node_healthy(&env, BOUNDARY_NODE_NAME);
    }
}

fn find_subnet(
    topology_snapshot: &TopologySnapshot,
    subnet_type: Option<SubnetType>,
    skip: Vec<SubnetId>,
) -> SubnetSnapshot {
    match subnet_type {
        None => topology_snapshot.root_subnet(),
        Some(subnet_type) => topology_snapshot
            .subnets()
            .find(|s| s.subnet_type() == subnet_type && !skip.contains(&s.subnet_id))
            .unwrap(),
    }
}

pub fn test_subnet(
    env: TestEnv,
    use_bn: bool,
    http_requests: bool,
    test_subnet_type: Option<SubnetType>,
    peer_subnet_type: Option<SubnetType>,
    excluded_tests: Vec<&str>,
    included_tests: Vec<&str>,
) {
    let log = env.logger();
    let topology_snapshot = &env.topology_snapshot();
    let test_subnet = find_subnet(topology_snapshot, test_subnet_type, vec![]);
    let peer_subnet = find_subnet(
        topology_snapshot,
        peer_subnet_type,
        vec![test_subnet.subnet_id],
    );
    let httpbin = if http_requests {
        let webserver_ipv6 = get_universal_vm_address(&env);
        Some(format!("[{webserver_ipv6}]:20443"))
    } else {
        None
    };
    let ic_ref_test_path = env
        .get_dependency_path("rs/tests/ic-hs/bin/ic-ref-test")
        .into_os_string()
        .into_string()
        .unwrap();
    let mut all_excluded_tests = excluded_tests;
    all_excluded_tests.append(&mut EXCLUDED.to_vec());
    with_endpoint(
        env,
        test_subnet,
        peer_subnet,
        use_bn,
        httpbin,
        ic_ref_test_path,
        log,
        all_excluded_tests,
        included_tests,
    );
}

fn subnet_config(subnet: &SubnetSnapshot) -> String {
    format!(
        "(\"{}\",{},[{}],[{}],[{}])",
        subnet.subnet_id,
        match subnet.subnet_type() {
            SubnetType::VerifiedApplication => "verified_application",
            SubnetType::Application => "application",
            SubnetType::System => "system",
        },
        subnet
            .nodes()
            .map(|n| format!("\"{}\"", n.node_id))
            .collect::<Vec<String>>()
            .join(","),
        subnet
            .subnet_canister_ranges()
            .iter()
            .map(|r| format!(
                "({},{})",
                canister_id_into_u64(r.start),
                canister_id_into_u64(r.end)
            ))
            .collect::<Vec<String>>()
            .join(","),
        subnet
            .nodes()
            .map(|n| format!("\"{}\"", n.get_public_url()))
            .collect::<Vec<String>>()
            .join(",")
    )
}

pub fn with_endpoint(
    env: TestEnv,
    test_subnet: SubnetSnapshot,
    peer_subnet: SubnetSnapshot,
    use_bn: bool,
    httpbin: Option<String>,
    ic_ref_test_path: String,
    log: Logger,
    excluded_tests: Vec<&str>,
    included_tests: Vec<&str>,
) {
    let endpoint = if use_bn {
        let boundary_node = env
            .get_deployed_boundary_node(BOUNDARY_NODE_NAME)
            .unwrap()
            .get_snapshot()
            .unwrap();
        boundary_node.get_public_url().to_string()
    } else {
        test_subnet
            .nodes()
            .next()
            .unwrap()
            .get_public_url()
            .to_string()
    };
    let test_subnet_config = subnet_config(&test_subnet);
    let peer_subnet_config = subnet_config(&peer_subnet);
    info!(log, "test-subnet-config: {}", test_subnet_config);
    info!(log, "peer-subnet-config: {}", peer_subnet_config);
    let mut cmd = Command::new(ic_ref_test_path);
    cmd.env(
        "IC_TEST_DATA",
        env.get_dependency_path("rs/tests/ic-hs/test-data"),
    )
    .arg("-j16")
    .arg("--pattern")
    .arg(tests_to_pattern(excluded_tests, included_tests))
    .arg("--endpoint")
    .arg(endpoint)
    .arg("--test-subnet-config")
    .arg(test_subnet_config)
    .arg("--peer-subnet-config")
    .arg(peer_subnet_config)
    .arg("--allow-self-signed-certs")
    .arg("True");
    if let Some(httpbin) = httpbin {
        cmd.arg("--httpbin").arg(&httpbin);
    }
    let status = cmd
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("ic-ref-test binary crashed");
    info!(log, "{}", format!("Status of ic-ref-test: {:?}", &status));
    assert!(status.success());
}

fn tests_to_pattern(excluded_tests: Vec<&str>, included_tests: Vec<&str>) -> String {
    let excluded = format!("!({})", excluded_tests.join(" || "));
    if included_tests.is_empty() {
        excluded
    } else {
        let included = format!("({})", included_tests.join(" || "));
        format!("{} && {}", excluded, included)
    }
}
