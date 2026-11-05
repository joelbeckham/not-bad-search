using System.Runtime.InteropServices;
using Microsoft.CommandPalette.Extensions;
using Shmuelie.WinRTServer;
using Shmuelie.WinRTServer.CsWinRT;

namespace GoodSearch.CommandPalette;

internal static class Program
{
    [MTAThread]
    public static void Main(string[] args)
    {
        if (!args.Contains("-RegisterProcessAsComServer")) return;
        using var disposed = new ManualResetEvent(false);
        var extension = new GoodSearchExtension(disposed);
        var server = new ComServer();
        server.RegisterClass<GoodSearchExtension, IExtension>(() => extension);
        server.Start();
        disposed.WaitOne();
        server.Stop();
        server.UnsafeDispose();
    }
}

[Guid("304E55FC-AE41-4D23-8237-67116778812B")]
public sealed partial class GoodSearchExtension(ManualResetEvent disposed) : IExtension, IDisposable
{
    private readonly GoodSearchProvider _provider = new();
    public object? GetProvider(ProviderType providerType) => providerType == ProviderType.Commands ? _provider : null;
    public void Dispose() { _provider.Dispose(); disposed.Set(); }
}
