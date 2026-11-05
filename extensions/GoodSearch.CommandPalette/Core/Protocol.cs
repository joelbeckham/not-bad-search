using System.Text.Json;

namespace GoodSearch.Core;

public sealed record SearchHit(string Path, string Name, Snippet[]? Snippets = null);
public sealed record Snippet(int Line, string Text);
public sealed record SearchResult(SearchHit[] Hits);
public sealed record PreviewResult(string Kind, string? Text = null, string? ImagePath = null,
    int StartLine = 1, bool Truncated = false, string? Message = null);
public sealed record SearchRequest(int Version, string Id, string Op, string Query, int Limit);
public sealed record PreviewRequest(int Version, string Id, string Op, string Path, string Query,
    int MaxBytes, int ContextLines);
public sealed record RpcError(string Code, string Message);
public sealed record RpcResponse(int Version, string Id, bool Ok, JsonElement Result, RpcError? Error);

public interface ISearchBackend
{
    Task<SearchResult> SearchAsync(string query, CancellationToken cancellationToken);
    Task<PreviewResult> PreviewAsync(string path, string query, CancellationToken cancellationToken);
}

public sealed class BackendException(string message) : Exception(message);

public static class Protocol
{
    public static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web)
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        PropertyNameCaseInsensitive = false,
    };
}
