use criterion_bencher_compat::{benchmark_group, benchmark_main, Bencher};
mod common;
use common::*;

/// benchmark functions for `amqprs` client
mod client_amqprs {
    use super::{get_size_list, rt, setup_tracing, Bencher};
    use amqprs::{
        callbacks::{DefaultChannelCallback, DefaultConnectionCallback},
        channel::{
            BasicPublishArguments, QueueBindArguments, QueueDeclareArguments, QueuePurgeArguments,
        },
        connection::{Connection, OpenConnectionArguments},
        BasicProperties,
    };

    pub fn amqprs_basic_pub(bencher: &mut Bencher) {
        setup_tracing();
        let rt = rt();

        // open a connection to RabbitMQ server
        let connection = rt.block_on(async {
            let connection = Connection::open(&OpenConnectionArguments::new(
                "localhost",
                5672,
                "user",
                "bitnami",
            ))
            .await
            .unwrap();
            connection
                .register_callback(DefaultConnectionCallback)
                .await
                .unwrap();
            connection
        });

        // open a channel on the connection
        let channel = rt.block_on(async {
            let channel = connection.open_channel(None).await.unwrap();
            channel
                .register_callback(DefaultChannelCallback)
                .await
                .unwrap();
            channel
        });

        //////////////////////////////////////////////////////////////////////////////
        // publish message
        let rounting_key = "bench.amqprs.pub";
        let exchange_name = "amq.topic";
        let queue_name = "bench-amqprs-q";
        rt.block_on(async {
            // declare a queue
            let (_, _, _) = channel
                .queue_declare(QueueDeclareArguments::new(queue_name))
                .await
                .unwrap()
                .unwrap();
            // bind queue to exchange
            channel
                .queue_bind(QueueBindArguments::new(
                    queue_name,
                    exchange_name,
                    rounting_key,
                ))
                .await
                .unwrap();
        });

        let pubargs = BasicPublishArguments::new(exchange_name, rounting_key);
        let declargs = QueueDeclareArguments::new(queue_name)
            .passive(true)
            .finish();

        let msg_size_list = get_size_list(connection.frame_max() as usize);
        // task to be benchmarked
        let task = || async {
            let count = msg_size_list.len();
            // purge queue
            channel
                .queue_purge(QueuePurgeArguments::new(queue_name))
                .await
                .unwrap();
            let (_, msg_cnt, _) = channel
                .queue_declare(
                    QueueDeclareArguments::new(queue_name)
                        .passive(true)
                        .finish(),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(0, msg_cnt);
            // publish  messages of variable sizes
            for i in 0..count {
                channel
                    .basic_publish(
                        BasicProperties::default(),
                        vec![0xc5; msg_size_list[i]],
                        pubargs.clone(),
                    )
                    .await
                    .unwrap();
            }
            // check all messages arrived at queue
            loop {
                let (_, msg_cnt, _) = channel
                    .queue_declare(declargs.clone())
                    .await
                    .unwrap()
                    .unwrap();
                if count == msg_cnt as usize {
                    break;
                }
            }
        };
        // start benchmark
        bencher.iter(|| {
            rt.block_on(task());
        });
        // explicitly close
        rt.block_on(async {
            channel.close().await.unwrap();
            connection.close().await.unwrap();
        });
    }
}

/// benchmark functions for `lapin` client
mod client_lapin {
    use super::{get_size_list, rt, setup_tracing, Bencher};
    use lapin::{
        options::{BasicPublishOptions, QueueBindOptions, QueueDeclareOptions, QueuePurgeOptions},
        types::FieldTable,
        BasicProperties, Connection, ConnectionProperties,
    };
    use tokio_executor_trait::Tokio;

    pub fn lapin_basic_pub(bencher: &mut Bencher) {
        setup_tracing();

        let rt = rt();

        let uri = "amqp://user:bitnami@localhost:5672";
        let options = ConnectionProperties::default()
            // Use tokio executor and reactor.
            // At the moment the reactor is only available for unix.
            .with_executor(Tokio::default().with_handle(rt.handle().clone()))
            .with_reactor(tokio_reactor_trait::Tokio);

        let (connection, channel) = rt.block_on(async {
            let connection = Connection::connect(uri, options).await.unwrap();
            let channel = connection.create_channel().await.unwrap();

            (connection, channel)
        });

        let rounting_key = "bench.lapin.pub";
        let exchange_name = "amq.topic";
        let queue_name = "bench-lapin-q";

        rt.block_on(async {
            channel
                .queue_declare(
                    queue_name,
                    QueueDeclareOptions::default(),
                    FieldTable::default(),
                )
                .await
                .unwrap();
            channel
                .queue_bind(
                    queue_name,
                    exchange_name,
                    rounting_key,
                    QueueBindOptions::default(),
                    FieldTable::default(),
                )
                .await
                .unwrap();
            channel
                .queue_bind(
                    queue_name,
                    exchange_name,
                    rounting_key,
                    QueueBindOptions::default(),
                    FieldTable::default(),
                )
                .await
                .unwrap();
        });

        let pubopts = BasicPublishOptions::default();
        let mut declopts = QueueDeclareOptions::default();
        declopts.passive = true;

        let msg_size_list = get_size_list(connection.configuration().frame_max() as usize);

        let task = || async {
            let count = msg_size_list.len();
            // purge queue
            channel
                .queue_purge(queue_name, QueuePurgeOptions::default())
                .await
                .unwrap();
            let q_state = channel
                .queue_declare(queue_name, declopts, FieldTable::default())
                .await
                .unwrap();

            assert_eq!(0, q_state.message_count());
            // publish  messages of variable sizes
            for i in 0..count {
                let _confirm = channel
                    .basic_publish(
                        exchange_name,
                        rounting_key,
                        pubopts,
                        &vec![0xc5; msg_size_list[i]],
                        BasicProperties::default(),
                    )
                    .await
                    .unwrap()
                    .await
                    .unwrap();
            }
            // check all messages arrived at queue
            loop {
                let q_state = channel
                    .queue_declare(queue_name, declopts, FieldTable::default())
                    .await
                    .unwrap();
                if count == q_state.message_count() as usize {
                    break;
                }
            }
        };
        // start benchmark
        bencher.iter(|| {
            rt.block_on(task());
        });

        rt.block_on(async {
            channel.close(0, "").await.unwrap();
            connection.close(0, "").await.unwrap();
        });
    }
}

benchmark_group!(amqprs, client_amqprs::amqprs_basic_pub,);
benchmark_group!(lapin, client_lapin::lapin_basic_pub,);

benchmark_main!(amqprs, lapin);
