//INCLUDEFILE ble-trouble
use esp_wifi::ble::controller::BleConnector;
use static_cell::StaticCell;
use trouble_host::prelude::*;
//IF option("defmt")
//IF !option("probe-rs")
//+ use esp_println as _;
//ENDIF
//+ use defmt::info;
//ELIF option("log")
use log::info;
//ENDIF probe-rs

/// Maximum number of connections
const CONN_MAX: usize = 1;

/// Max number of L2CAP channels.
const L2CAP_CHANNELS_MAX: usize = 2; // Signal + att

/// Max L2CAP MTU size.
const L2CAP_MTU: usize = 256;

const SLOTS: usize = 20;

pub type BleController = bt_hci::controller::ExternalController<BleConnector<'static>, SLOTS>;

type BleResources = HostResources<CONN_MAX, L2CAP_CHANNELS_MAX, L2CAP_MTU>;

#[gatt_service(uuid = service::ENVIRONMENTAL_SENSING)]
pub struct AmbientService {
    #[descriptor(uuid = descriptors::MEASUREMENT_DESCRIPTION, read, value = "Temperature °C")]
    #[characteristic(uuid = characteristic::TEMPERATURE, read, notify)]
    pub temperature: i16,
    #[descriptor(uuid = descriptors::MEASUREMENT_DESCRIPTION, read, value = "Humidity %")]
    #[characteristic(uuid = characteristic::HUMIDITY, read, notify)]
    pub humidity: i16,
}

#[gatt_service(uuid = "911fd452-297b-408f-8f53-ada4e57647df")]
pub struct CounterService {
    #[characteristic(uuid = "d1ab4642-d3f0-4a1c-ae44-e01012d85a13", read, notify, write)]
    pub count: u8,
}

#[gatt_server]
pub struct GattServer {
    pub ambient: AmbientService,
    pub counter: CounterService,
}

#[embassy_executor::task]
async fn ble_task(mut runner: Runner<'static, BleController>) {
    runner.run().await.expect("Error in BLE task");
}

/// Create an advertiser to use to connect to a BLE Central, and wait for it to connect.
pub async fn advertise<'server, 'values, C: Controller>(
    name: &'values str,
    peripheral: &mut Peripheral<'values, C>,
    server: &'server GattServer<'values>,
) -> Result<GattConnection<'values, 'server>, BleHostError<C::Error>> {
    let mut advertiser_data = [0; 31];
    AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::ServiceUuids16(&[[0x0f, 0x18]]),
            AdStructure::CompleteLocalName(name.as_bytes()),
        ],
        &mut advertiser_data[..],
    )?;
    let advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &advertiser_data[..],
                scan_data: &[],
            },
        )
        .await?;
    //IF option("defmt") || option("log")
    info!("[adv] advertising");
    //ENDIF
    let conn = advertiser.accept().await?.with_attribute_server(server)?;
    //IF option("defmt") || option("log")
    info!("[adv] connection established");
    //ENDIF
    Ok(conn)
}

impl<'values> GattServer<'values> {
    /// Build the stack for the GATT server and start background tasks required.
    pub fn start(
        name: &'values str,
        appearance: impl Into<&'static BluetoothUuid16>,
        spawner: embassy_executor::Spawner,
        controller: BleController,
    ) -> (&'static Self, Peripheral<'values, BleController>) {
        // Using a fixed "random" address can be useful for testing. In real scenarios, one would
        // use e.g. the MAC 6 byte array as the address (how to get that varies by the platform).
        let address = Address::random([0x42, 0x5A, 0xE3, 0x1E, 0x83, 0xE8]);
        //IF option("log")
        info!("Our address = {:?}", address);
        //ENDIF

        let resources = {
            static RESOURCES: StaticCell<BleResources> = StaticCell::new();
            RESOURCES.init(BleResources::new())
        };
        let stack = {
            static STACK: StaticCell<Stack<'_, BleController>> = StaticCell::new();
            STACK.init(trouble_host::new(controller, resources).set_random_address(address))
        };
        let host = stack.build();
        let server = {
            static SERVER: StaticCell<GattServer<'_>> = StaticCell::new();
            SERVER.init(
                GattServer::new_with_config(GapConfig::Peripheral(PeripheralConfig {
                    name,
                    appearance: appearance.into(),
                }))
                .expect("Error creating Gatt Server"),
            )
        };
        //IF option("defmt") || option("log")
        info!("Starting Gatt Server");
        //ENDIF
        spawner.must_spawn(ble_task(host.runner));
        (server, host.peripheral)
    }

    /// Background task to process BLE IO events.
    pub async fn start_task<'server>(
        &self,
        conn: &GattConnection<'values, 'server>,
    ) -> Result<(), trouble_host::Error> {
        let reason = loop {
            match conn.next().await {
                GattConnectionEvent::Disconnected { reason } => break reason,
                GattConnectionEvent::Gatt { event: Err(_e) } => {
                    //IF option("defmt") || option("log")
                    info!("[gatt] error processing event")
                    //ENDIF
                }
                GattConnectionEvent::Gatt { event: Ok(event) } => {
                    match &event {
                        GattEvent::Read(event) => {
                            //IF option("defmt") || option("log")
                            info!("[gatt] Read event occured for handle: {}", event.handle());
                            //ENDIF
                        }
                        GattEvent::Write(event) => {
                            //IF option("defmt") || option("log")
                            info!("[gatt] Write event occured for handle: {}", event.handle());
                            //ENDIF
                        }
                    }
                    match event.accept() {
                        Ok(reply) => reply.send().await,
                        Err(_e) => {
                            //IF option("defmt") || option("log")
                            info!("[gatt] error sending response");
                            //ENDIF
                        }
                    }
                }
                _ => {} // ignore other events
            }
        };
        //IF option("defmt") || option("log")
        info!("[gatt] disconnected: {:?}", reason);
        //ENDIF
        Ok(())
    }
}
