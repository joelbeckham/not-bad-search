using System.IO.Pipes;
using System.Text.Json;

namespace GoodSearch.Core;

// Per-call connections isolate cancellation and concurrent searches/previews.
public sealed class PipeBackend(string pipeName = "good-search-v1", TimeSpan? timeout = null) : ISearchBackend
{
    public const int MaxFrameBytes = 1_048_576;
    private readonly TimeSpan _timeout = timeout ?? TimeSpan.FromSeconds(5);
    public Task<SearchResult> SearchAsync(string query, CancellationToken cancellationToken)
    {
        var id = Guid.NewGuid().ToString("N");
        return CallAsync<SearchRequest, SearchResult>(new(1, id, "search", query, 100), id, cancellationToken);
    }
    public Task<PreviewResult> PreviewAsync(string path, string query, CancellationToken cancellationToken)
    {
        var id = Guid.NewGuid().ToString("N");
        return CallAsync<PreviewRequest, PreviewResult>(new(1, id, "preview", path, query, 65_536, 3), id, cancellationToken);
    }
    private async Task<TResponse> CallAsync<TRequest, TResponse>(TRequest request, string id, CancellationToken token)
    {
        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(token);
        deadline.CancelAfter(_timeout);
        try
        {
            using var pipe = new NamedPipeClientStream(".", pipeName, PipeDirection.InOut, PipeOptions.Asynchronous);
            await pipe.ConnectAsync(deadline.Token).ConfigureAwait(false);
            var bytes = JsonSerializer.SerializeToUtf8Bytes(request, Protocol.Json);
            if (bytes.Length > MaxFrameBytes) throw new BackendException("Request exceeds the IPC size limit.");
            await pipe.WriteAsync(bytes, deadline.Token).ConfigureAwait(false);
            await pipe.WriteAsync(new byte[] { 10 }, deadline.Token).ConfigureAwait(false);
            await pipe.FlushAsync(deadline.Token).ConfigureAwait(false);
            using var frame = new MemoryStream();
            var buffer = new byte[4096];
            while (true)
            {
                var count = await pipe.ReadAsync(buffer, deadline.Token).ConfigureAwait(false);
                if (count == 0) throw new BackendException("Backend closed the connection before completing its response.");
                var newline = Array.IndexOf(buffer, (byte)10, 0, count);
                var length = newline < 0 ? count : newline;
                if (frame.Length + length > MaxFrameBytes) throw new BackendException("Backend response exceeds the IPC size limit.");
                frame.Write(buffer, 0, length);
                if (newline >= 0) break;
            }
            var reply = JsonSerializer.Deserialize<RpcResponse>(frame.ToArray(), Protocol.Json)
                ?? throw new BackendException("Backend returned an empty response.");
            if (reply.Version != 1 || reply.Id != id) throw new BackendException("Backend protocol version or request ID did not match.");
            if (!reply.Ok) throw new BackendException($"{reply.Error?.Code ?? "backend_error"}: {reply.Error?.Message ?? "Request failed"}");
            return reply.Result.Deserialize<TResponse>(Protocol.Json)
                ?? throw new BackendException("Backend returned an empty result.");
        }
        catch (OperationCanceledException) when (!token.IsCancellationRequested)
        { throw new BackendException("Good Search backend did not respond within the timeout. Start the Rust IPC backend and try again."); }
        catch (JsonException)
        { throw new BackendException("Backend returned invalid JSON or an incompatible response."); }
        catch (IOException ex)
        { throw new BackendException($"Cannot communicate with Good Search: {ex.Message}"); }
    }
}
