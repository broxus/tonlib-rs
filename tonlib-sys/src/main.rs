use ton_api::ton;
use tonlib_sys::TonlibClient;

fn main() {
    let result = TonlibClient::execute(&ton::rpc::GenerateKeyPair {
        word_count: 24,
        password: ton_api::secure::SecureString::new(b"Hello world".to_vec()),
        entropy: ton_api::secure::SecureString::new(b"Entropy".to_vec()),
    });

    println!("{:?}", result);
}
