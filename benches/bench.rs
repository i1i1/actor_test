use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

pub mod test_actix {
    use actix::prelude::*;
    use criterion::black_box;
    use std::future::Future;

    pub struct ActixRunner(pub actix::SystemRunner);

    impl ActixRunner {
        pub fn new() -> Self {
            Self(System::new())
        }

        pub fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
            self.0.block_on(future)
        }
    }

    impl criterion::async_executor::AsyncExecutor for &ActixRunner {
        fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
            self.0.block_on(future)
        }
    }

    #[derive(Message)]
    #[rtype(result = "()")]
    pub struct Ping<const SIZE: usize>([u8; SIZE]);

    #[derive(Clone)]
    pub struct PingActor(Option<Addr<PingActor>>);

    impl Actor for PingActor {
        type Context = Context<Self>;
    }

    impl<const SIZE: usize> Handler<Ping<SIZE>> for PingActor {
        type Result = ResponseActFuture<Self, ()>;

        fn handle(&mut self, msg: Ping<SIZE>, _: &mut Context<Self>) -> Self::Result {
            if let Some(addr) = &self.0 {
                let future = actix::fut::wrap_future::<_, Self>(addr.send(msg));
                Box::pin(future.map(|result, _actor, _ctx| result.unwrap()))
            } else {
                let future = actix::fut::wrap_future::<_, Self>(async {});
                Box::pin(future)
            }
        }
    }

    pub async fn start_actors(n: usize, _: &ActixRunner) -> Vec<Addr<PingActor>> {
        let mut prev = None;
        let mut actors = Vec::with_capacity(n);
        for _ in 0..n {
            let addr = PingActor(prev.clone()).start();
            actors.push(addr.clone());
            prev = Some(addr);
        }
        actors
    }

    pub async fn test<const SIZE: usize>(actors: &[Addr<PingActor>]) {
        actors
            .last()
            .unwrap()
            .send(black_box(Ping([0; SIZE])))
            .await
            .unwrap();
    }
}

pub mod test_xtra {
    use criterion::black_box;
    use xtra::prelude::*;

    pub struct Ping<const SIZE: usize>([u8; SIZE]);

    impl<const SIZE: usize> Message for Ping<SIZE> {
        type Result = ();
    }

    #[derive(Clone)]
    pub struct PingActor(Option<Address<PingActor>>);

    impl Actor for PingActor {}

    #[async_trait::async_trait]
    impl<const SIZE: usize> Handler<Ping<SIZE>> for PingActor {
        async fn handle(&mut self, msg: Ping<SIZE>, _: &mut Context<Self>) {
            if let Some(addr) = &self.0 {
                addr.send(msg).await.unwrap()
            }
        }
    }

    pub async fn start_actors(
        n: usize,
        runtime: &tokio::runtime::Runtime,
    ) -> Vec<Address<PingActor>> {
        let mut prev = None;
        let mut actors = Vec::with_capacity(n);
        for _ in 0..n {
            let addr = PingActor(prev.clone())
                .create(None)
                .spawn(&mut xtra::spawn::Tokio::Handle(runtime));
            actors.push(addr.clone());
            prev = Some(addr);
        }
        actors
    }

    pub async fn test<const SIZE: usize>(actors: &[Address<PingActor>]) {
        actors
            .last()
            .unwrap()
            .send(black_box(Ping([0; SIZE])))
            .await
            .unwrap();
    }
}

mod test_iroha {
    use criterion::black_box;
	use iroha_actor::*;
    use std::future::Future;

	pub struct AsyncStdExecutor;

    impl criterion::async_executor::AsyncExecutor for &AsyncStdExecutor {
        fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
            async_std::task::block_on(future)
        }
    }

    impl criterion::async_executor::AsyncExecutor for AsyncStdExecutor {
        fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
            async_std::task::block_on(future)
        }
    }

    pub struct Ping<const SIZE: usize>([u8; SIZE]);

    impl<const SIZE: usize> Message for Ping<SIZE> {
        type Result = ();
    }

    #[derive(Clone)]
    pub struct PingActor(Option<Addr<PingActor>>);

    impl Actor for PingActor {}

    #[async_trait::async_trait]
    impl<const SIZE: usize> Handler<Ping<SIZE>> for PingActor {
		type Result = ();

        async fn handle(&mut self, msg: Ping<SIZE>) {
            if let Some(addr) = &mut self.0 {
                addr.send(msg).await.unwrap()
            }
        }
    }

    pub async fn start_actors(
        n: usize,
        _runtime: &AsyncStdExecutor,
    ) -> Vec<Addr<PingActor>> {
        let mut prev = None;
        let mut actors = Vec::with_capacity(n);
        for _ in 0..n {
            let addr = PingActor(prev.clone()).start();
            actors.push(addr.clone());
            prev = Some(addr);
        }
        actors
    }

    pub async fn test<const SIZE: usize>(actors: &[Addr<PingActor>]) {
        actors
            .last()
            .unwrap()
			.clone()
            .send(black_box(Ping([0; SIZE])))
            .await
            .unwrap();
    }
}

macro_rules! bench(
	( $bencher:ident, $name:literal, $runner:ident, $i:ident, $($n:literal,)* ) => {$(
        $bencher.bench_with_input(
            BenchmarkId::new(&format!("{} {}", $name, $i), $n),
            &(),
            |b, _| {
                let actors = $runner.block_on(start_actors($i, &$runner));
                b.to_async($runner)
                    .iter(|| test::< $n >(&actors))
            },
        );
	)*}
);

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("Actors");

	{
		use criterion::async_executor::AsyncExecutor;
		use test_iroha::*;

		for &i in [1, 2, 4, 8].iter() {
			bench!(group, "iroha", AsyncStdExecutor, i, 0, 1, 4, 16, 64, 256, 1024, 4096,);
		}
	}

	{
		use test_actix::*;

		let runner = &ActixRunner::new();

		for &i in [1, 2, 4, 8].iter() {
			bench!(group, "actix", runner, i, 0, 1, 4, 16, 64, 256, 1024, 4096,);
		}
	}

	{
		use test_xtra::*;

		let tokio = &tokio::runtime::Runtime::new().unwrap();

		for &i in [1, 2, 4, 8].iter() {
			bench!(group, "xtra", tokio, i, 0, 1, 4, 16, 64, 256, 1024, 4096,);
		}
	}
}

criterion_group!(benches, bench);
criterion_main!(benches);
