use alsa;
use alsa::seq;
use std::env;
use std::str::FromStr;

mod config;
mod pipeline;

fn main() {
    println!("opening sequencer");
    let my_client = seq::Seq::open(None, None, false)
        .expect("wanted to open the sequencer");
    for client in seq::ClientIter::new(&my_client) {
        for port in seq::PortIter::new(&my_client, client.get_client()) {
            if !port.get_type().contains(seq::PortType::PORT) { continue }
            if !port.get_type().contains(seq::PortType::MIDI_GENERIC) { continue }
            let addr_str = format!("{}:{}", port.get_client(), port.get_port());
            let flags: Vec<_> = port.get_capability().iter()
                .filter_map(|cap| match cap {
                    seq::PortCap::READ  => Some("R"),
                    seq::PortCap::WRITE => Some("W"),
                    _ => None
                })
                .collect();
            println!("port {} '{}' is type {:?} with caps {}", addr_str, port.get_name().unwrap_or("?"), port.get_type(), flags.join("/"));
        }
    }

    let args: Vec<_> = env::args().collect();
    let cfgfn = &args[1];

    let config = match config::Config::read(&cfgfn) {
        Ok(cfg) => {
            println!("yay! successfully read {}", cfg.filename);
            for dev in cfg.devices.iter() {
                println!("device {}:", dev.name);
                if let Some(name) = &dev.input {
                    println!("- input: {}", name);
                }
                if let Some(name) = &dev.output {
                    println!("- output: {}", name);
                }
            }
            for route in cfg.routes.iter() {
                let state = match route.enabled {
                    true => "active",
                    false => "inactive",
                };
                println!("{} route: {} -> {}", state, route.source, route.sink);
            }
            cfg
        },
        Err(why) => panic!("oh no! {}", why),
    };

    let mut pipelines: Vec<_> = config.routes.iter()
        .filter(|route| route.enabled)
        .filter_map(|route| {
            let Some(sourcedev) = config.get_device(&route.source) else {
                println!("roujte '{0}' to '{1}': could not find source device '{0}'", route.source, route.sink);
                return None;
            };
            let Some(sinkdev) = config.get_device(&route.sink) else {
                println!("roujte '{0}' to '{1}': could not find sink device '{1}'", route.source, route.sink);
                return None;
            };

            let Some(ref sourceaddrstr) = sourcedev.input else {
                println!("route '{0}' to '{1}': no input address for device '{0}'", route.source, route.sink);
                return None;
            };
            let Some(ref sinkaddrstr) = sinkdev.output else {
                println!("route '{0}' to '{1}': no output address for device '{1}'", route.source, route.sink);
                return None;
            };

            let Ok(sourceaddr) = seq::Addr::from_str(&sourceaddrstr) else {
                println!("device '{0}': unable to parse address '{1}'", sourcedev.name, sourceaddrstr);
                return None;
            };
            let Ok(sinkaddr) = seq::Addr::from_str(&sinkaddrstr) else {
                println!("device '{0}': unable to parse address '{1}'", sinkdev.name, sinkaddrstr);
                return None;
            };

            let mut filters: Vec<_> = Vec::new();
            filters.extend_from_slice(sourcedev.input_filters.as_slice());
            filters.extend_from_slice(route.filters.as_slice());
            filters.extend_from_slice(sinkdev.output_filters.as_slice());

            match pipeline::Pipeline::new(sourceaddr, sinkaddr, &filters) {
                Ok(pl) => {
                    println!("constructed pipeline for route '{0}' to '{1}'", route.source, route.sink);
                    Some(pl)
                },
                Err(why) => {
                    println!("route '{0}' to '{1}': {2}", route.source, route.sink, why);
                    None
                }
            }
        })
        .collect();
    
    println!("now running {} pipelines, Ctrl-C to stop", pipelines.len());
    loop {
        let stats: Vec<_> = pipelines.iter_mut()
            .map(|pl| {
                if let Err(why) = pl.run() {
                    println!("{}", why);
                }
                pl.get_stats()
            })
            .enumerate()
            .map(|(idx, stats)| {
                format!("{0} [{1:6} | {2:6}]", idx, stats.ingested, stats.delivered)
            })
            .collect();
        print!("\r{}", stats.join("  / "));
    }
}
