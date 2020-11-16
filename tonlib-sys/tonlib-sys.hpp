#pragma once

#include <auto/tl/tonlib_api.h>
#include <td/actor/actor.h>

namespace trs {
namespace tonlib_api = ton::tonlib_api;

class Client final {
public:
  using Request = tonlib_api::object_ptr<tonlib_api::Function>;
  using Response = tonlib_api::object_ptr<tonlib_api::Object>;

  Client();

  void send(Request &&request, td::Promise<Response> &&response);
  static Response execute(Request &&request);

  ~Client();
  Client(Client &&other) noexcept;
  Client &operator=(Client &&other) noexcept;

private:
  class Impl;
  std::unique_ptr<Impl> impl_;
};

} // namespace trs

extern "C" {

struct ExecutionResult {
  void *data_ptr;
  uint64_t data_len;
};

auto trs_create_client() -> void *;
void trs_delete_client(void *client_ptr);
void trs_run(void *client_ptr, const void *query_ptr, uint64_t query_len);
auto trs_execute(const void *query_ptr, uint64_t query_len) -> ExecutionResult;
void trs_delete_response(const ExecutionResult *response);
}
