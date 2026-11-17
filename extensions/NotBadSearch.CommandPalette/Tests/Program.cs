using System.IO.Pipes;
using System.Text;
using System.Text.Json;
using NotBadSearch.Core;

static void Assert(bool value, string message) { if (!value) throw new Exception(message); }
static async Task Throws<T>(Func<Task> action) where T : Exception
{
    try { await action(); } catch (T) { return; }
    throw new Exception($"Expected {typeof(T).Name}");
}
static async Task Exchange(Func<JsonElement, byte[]> response, Func<PipeBackend, Task> verify, bool fragmented = false)
{
    var name = "not-bad-search-test-" + Guid.NewGuid().ToString("N");
    using var server = new NamedPipeServerStream(name, PipeDirection.InOut, 1, PipeTransmissionMode.Byte, PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
    using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(10));
    var serving = Task.Run(async () =>
    {
        await server.WaitForConnectionAsync(timeout.Token);
        using var reader = new StreamReader(server, new UTF8Encoding(false), leaveOpen: true);
        var request = await reader.ReadLineAsync(timeout.Token) ?? throw new Exception("Missing request");
        using var json = JsonDocument.Parse(request);
        var bytes = response(json.RootElement);
        if (fragmented)
        {
            foreach (var b in bytes) await server.WriteAsync(new byte[] { b }, timeout.Token);
        }
        else await server.WriteAsync(bytes, timeout.Token);
        await server.FlushAsync(timeout.Token);
        server.Disconnect();
    });
    await verify(new PipeBackend(name, TimeSpan.FromSeconds(3)));
    await serving;
}
static byte[] Reply(JsonElement request, object result, int version = 1, string? id = null) =>
    Encoding.UTF8.GetBytes(JsonSerializer.Serialize(new { version, id = id ?? request.GetProperty("id").GetString(), ok = true, result }, Protocol.Json) + "\n");

await Exchange(request =>
{
    Assert(request.GetProperty("op").GetString() == "search", "Search operation");
    Assert(request.GetProperty("query").GetString() == "café\n\"notes", "Escaped Unicode query round trip");
    Assert(request.GetProperty("limit").GetInt32() == 100, "Result cap");
    return Reply(request, new SearchResult([new(@"C:\notes\café.md", "café.md", [new(4, "match")])]));
}, async client =>
{
    var result = await client.SearchAsync("café\n\"notes", default);
    Assert(result.Hits.Single().Name == "café.md", "Fragmented UTF-8 response");
    Assert(result.Hits[0].Snippets![0].Line == 4, "Snippet line");
}, fragmented: true);
Console.WriteLine("PASS search framing, escaping, Unicode, snippets");

await Exchange(request =>
{
    Assert(request.GetProperty("op").GetString() == "preview", "Preview operation");
    Assert(request.GetProperty("max_bytes").GetInt32() == 65536, "Preview bound");
    return Reply(request, new PreviewResult("text", "hello\nworld", StartLine: 8, Truncated: true));
}, async client =>
{
    var preview = await client.PreviewAsync(@"C:\notes\note.md", "hello", default);
    Assert(preview.StartLine == 8 && preview.Truncated, "Preview metadata");
});
Console.WriteLine("PASS preview request and response");

await Exchange(r => Reply(r, new SearchResult([]), id: "wrong"), c => Throws<BackendException>(async () => await c.SearchAsync("x", default)));
await Exchange(r => Reply(r, new SearchResult([]), version: 2), c => Throws<BackendException>(async () => await c.SearchAsync("x", default)));
await Exchange(_ => Encoding.UTF8.GetBytes("not-json\n"), c => Throws<BackendException>(async () => await c.SearchAsync("x", default)));
await Exchange(_ => Encoding.UTF8.GetBytes("{}"), c => Throws<BackendException>(async () => await c.SearchAsync("x", default)));
await Exchange(r => Encoding.UTF8.GetBytes(JsonSerializer.Serialize(new { version = 1, id = r.GetProperty("id").GetString(), ok = false, error = new { code = "not_indexed", message = "File outside configured roots" } }) + "\n"), c => Throws<BackendException>(async () => await c.SearchAsync("x", default)));
await Exchange(_ => Enumerable.Repeat((byte)'x', PipeBackend.MaxFrameBytes + 1).Append((byte)10).ToArray(), c => Throws<BackendException>(async () => await c.SearchAsync("x", default)));
Console.WriteLine("PASS mismatched IDs/versions, malformed JSON, premature EOF, errors, size limits");

var absent = "not-bad-search-absent-" + Guid.NewGuid().ToString("N");
await Throws<BackendException>(async () => await new PipeBackend(absent, TimeSpan.FromMilliseconds(150)).SearchAsync("x", default));
using (var cancel = new CancellationTokenSource(100))
    await Throws<OperationCanceledException>(async () => await new PipeBackend(absent).SearchAsync("x", cancel.Token));
Console.WriteLine("PASS unavailable backend timeout and caller cancellation");

var rendered = PreviewMarkdown.Render(@"C:\note.md", new("text", "![injected](https://example.com)\n```", StartLine: 9));
Assert(rendered.Contains("    " + "    9  ![injected]"), "Contents remain indented literal text");
Assert(PreviewMarkdown.Render(@"C:\x", new("image", ImagePath: "https://example.com/x.png")).Contains("Preview unavailable"), "No remote image previews");
Assert(PreviewMarkdown.Render(@"C:\x", new("image", ImagePath: @"C:\pictures\a(b).png")).Contains("%28"), "Image URI escaping");
Console.WriteLine("PASS literal preview rendering and image URI checks");
Console.WriteLine("All IPC core tests passed.");
