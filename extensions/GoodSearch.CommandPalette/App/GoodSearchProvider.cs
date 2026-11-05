using System.Text.Json;
using GoodSearch.Core;
using Microsoft.CommandPalette.Extensions;
using Microsoft.CommandPalette.Extensions.Toolkit;

namespace GoodSearch.CommandPalette;

internal sealed record ExtensionConfig(string PipeName = "good-search-v1", int TimeoutSeconds = 5)
{
    public static ExtensionConfig Load()
    {
        var path = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "GoodSearch", "command-palette.json");
        if (!File.Exists(path)) return new();
        var config = JsonSerializer.Deserialize<ExtensionConfig>(File.ReadAllText(path), Protocol.Json)
            ?? throw new InvalidDataException("Extension configuration is empty.");
        if (string.IsNullOrWhiteSpace(config.PipeName) || config.PipeName.IndexOfAny(['\\', '/', ':']) >= 0 || config.TimeoutSeconds is < 1 or > 60)
            throw new InvalidDataException("Use a local pipe name and timeout_seconds between 1 and 60.");
        return config;
    }
}

internal sealed partial class GoodSearchProvider : CommandProvider
{
    private readonly SearchPage? _page;
    private readonly ICommandItem[] _commands;
    public GoodSearchProvider()
    {
        Id = "GoodSearch.CommandPalette";
        DisplayName = "Good Search";
        Icon = new IconInfo("\uE721");
        try
        {
            var config = ExtensionConfig.Load();
            _page = new SearchPage(new PipeBackend(config.PipeName, TimeSpan.FromSeconds(config.TimeoutSeconds)));
            _commands = [new CommandItem(_page) { Title = "Good Search", Subtitle = "Search your Rust-maintained file index" }];
        }
        catch (Exception ex) when (ex is IOException or InvalidDataException or JsonException or UnauthorizedAccessException)
        { _commands = [new CommandItem(new NoOpCommand()) { Title = "Good Search configuration error", Subtitle = ex.Message }]; }
    }
    public override ICommandItem[] TopLevelCommands() => _commands;
    public override void Dispose() => _page?.Dispose();
}
