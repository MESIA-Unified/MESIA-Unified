use mesia_shader::HwStage;
use mesia_shader::emit::{HostShader, validator::Validator};
use std::{fs, path::PathBuf, process::Command};

/// Independent PC shader pair for the identical POSITION0 buffer and RGBA8 oracle.
pub fn reference_stages() -> (HostShader, HostShader) {
    let compiler = std::env::var_os("MESIA_PC_DXC")
        .map(PathBuf::from)
        .or_else(|| Validator::default_path().map(|p| p.with_file_name("dxc.exe")))
        .expect("PC DXC for the native reference");
    let directory = tempfile::tempdir().expect("temporary shader directory");
    let source = directory.path().join("quad.hlsl");
    fs::write(&source, "float4 vs(float4 p : POSITION) : SV_Position { return p; }\nfloat4 ps() : SV_Target { return float4(1,0,1,1); }").unwrap();
    let compile = |stage, entry, profile| {
        let path = directory.path().join(format!("{entry}.dxil"));
        let output = Command::new(&compiler)
            .args(["-E", entry, "-T", profile, "-Fo"])
            .arg(&path)
            .arg(&source)
            .output()
            .expect("run PC DXC");
        assert!(
            output.status.success(),
            "DXC {entry}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        HostShader {
            stage,
            dxil: fs::read(path).unwrap(),
            user_data: vec![],
            samplers: vec![],
            static_samplers: vec![],
            root_cbvs: vec![],
            root_srvs: vec![],
            root_uavs: vec![],
            root_indexed_views: vec![],
        }
    };
    (
        compile(HwStage::Vs, "vs", "vs_6_6"),
        compile(HwStage::Ps, "ps", "ps_6_6"),
    )
}
