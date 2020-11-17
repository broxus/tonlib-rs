#include "tonlib-sys.hpp"

#include <tl-utils/common-utils.hpp>
#include <tonlib/Stuff.h>
#include <tonlib/TonlibCallback.h>
#include <tonlib/TonlibClient.h>

namespace {
auto fetch_tl_function(const void *query_ptr, uint64_t query_len)
    -> td::Result<tonlib_api::object_ptr<tonlib_api::Function>> {
  td::BufferSlice data{reinterpret_cast<const char *>(query_ptr),
                       static_cast<size_t>(query_len)};

  td::TlBufferParser p(&data);
  using T = tonlib_api::Function;
  auto R = ton::move_tl_object_as<T>(T::fetch(p));
  p.fetch_end();
  if (p.get_status().is_ok()) {
    return std::move(R);
  } else {
    return p.get_status();
  }
}

auto store_tl_object(tonlib_api::object_ptr<tonlib_api::Object> &&object)
    -> ExecutionResult {
  const auto *T = object.get();

  td::TlStorerCalcLength X;
  T->store(X);
  auto l = X.get_length() + 4u;
  auto len = l;

  auto *ptr = new uint8_t[len];
  LOG(WARNING) << "allocating response [" << reinterpret_cast<intptr_t>(ptr)
               << ", " << len << "]";
  td::TlStorerUnsafe Y(ptr);
  Y.store_binary(T->get_id());
  T->store(Y);

  return ExecutionResult{ptr, len};
}

auto store_tl_object(td::Status &&status) {
  return store_tl_object(tonlib::status_to_tonlib_api(status.move_as_error()));
}

} // namespace

namespace trs {
class Client::Impl final {
public:
  Impl() {
    class Callback final : public tonlib::TonlibCallback {
    public:
      explicit Callback() = default;
      void on_result(std::uint64_t id,
                     tonlib_api::object_ptr<tonlib_api::Object> result) final {}
      void on_error(std::uint64_t id,
                    tonlib_api::object_ptr<tonlib_api::error> error) final {}
      Callback(const Callback &) = delete;
      Callback &operator=(const Callback &) = delete;
      Callback(Callback &&) = delete;
      Callback &operator=(Callback &&) = delete;
    };

    scheduler_.run_in_context([&] {
      tonlib_ = td::actor::create_actor<tonlib::TonlibClient>(
          td::actor::ActorOptions().with_name("Tonlib"),
          td::make_unique<Callback>());
    });
    scheduler_thread_ = td::thread([&] { scheduler_.run(); });
  }

  void send(Client::Request request, td::Promise<Client::Response> &&promise) {
    if (request == nullptr) {
      promise.set_error(td::Status::Error("Invalid request"));
      return;
    }

    scheduler_.run_in_context_external([&] {
      td::actor::send_closure(tonlib_, &tonlib::TonlibClient::request_async,
                              std::move(request), std::move(promise));
    });
  }

  Impl(const Impl &) = delete;
  Impl &operator=(const Impl &) = delete;
  Impl(Impl &&) = delete;
  Impl &operator=(Impl &&) = delete;
  ~Impl() {
    LOG(ERROR) << "~Impl";
    scheduler_.run_in_context_external([&] { tonlib_.reset(); });
    LOG(ERROR) << "Stop";
    scheduler_.run_in_context_external(
        [] { td::actor::SchedulerContext::get()->stop(); });
    LOG(ERROR) << "join";
    scheduler_thread_.join();
    LOG(ERROR) << "join - done";
  }

private:
  bool is_closed_{false};

  td::actor::Scheduler scheduler_{{1}};
  td::thread scheduler_thread_;
  td::actor::ActorOwn<tonlib::TonlibClient> tonlib_;
};

Client::Client() : impl_(std::make_unique<Impl>()) {}

void Client::send(Client::Request &&request, td::Promise<Response> &&response) {
  impl_->send(std::move(request), std::move(response));
}

Client::Response Client::execute(Client::Request &&request) {
  return tonlib::TonlibClient::static_request(std::move(request));
}

Client::~Client() = default;
Client::Client(Client &&other) noexcept = default;
Client &Client::operator=(Client &&other) noexcept = default;

} // namespace trs

extern "C" {

auto trs_create_client() -> void * {
  LOG(WARNING) << "trs_create_client";
  return static_cast<void *>(new trs::Client{});
}

void trs_delete_client(void *client_ptr) {
  LOG(WARNING) << "trs_delete_client(" << reinterpret_cast<intptr_t>(client_ptr)
               << ")";
  delete reinterpret_cast<const trs::Client *>(client_ptr);
}

void trs_run(void *client_ptr, const void *query_ptr, uint64_t query_len,
             Callback callback, void *context) {
  LOG(WARNING) << "trs_run(" << reinterpret_cast<intptr_t>(client_ptr) << ", "
               << reinterpret_cast<intptr_t>(query_ptr) << ", " << query_len
               << ", " << reinterpret_cast<intptr_t>(callback) << ", "
               << reinterpret_cast<intptr_t>(context) << ")";

  auto *client = reinterpret_cast<trs::Client *>(client_ptr);
  auto query = fetch_tl_function(query_ptr, query_len);
  if (query.is_error()) {
    return callback(context, store_tl_object(query.move_as_error()));
  }

  auto P = td::PromiseCreator::lambda(
      [callback, context](td::Result<trs::Client::Response> R) mutable {
        if (R.is_error()) {
          callback(context, store_tl_object(R.move_as_error()));
        } else {
          callback(context, store_tl_object(R.move_as_ok()));
        }
      });
  client->send(query.move_as_ok(), std::move(P));
}

auto trs_execute(const void *query_ptr, uint64_t query_len) -> ExecutionResult {
  LOG(WARNING) << "trs_execute(" << reinterpret_cast<intptr_t>(query_ptr)
               << ", " << query_len << ")";
  auto query = fetch_tl_function(query_ptr, query_len);
  if (query.is_error()) {
    return store_tl_object(query.move_as_error());
  } else {
    return store_tl_object(
        tonlib::TonlibClient::static_request(query.move_as_ok()));
  }
}

void trs_delete_response(const void *response_ptr) {
  LOG(WARNING) << "trs_delete_response("
               << reinterpret_cast<intptr_t>(response_ptr) << ")";
  delete[] reinterpret_cast<const char *>(response_ptr);
}
}
