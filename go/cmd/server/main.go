package main

import (
	"context"
	"log/slog"
	"net"
	"os"
	"os/signal"

	"google.golang.org/grpc"

	"go.opentelemetry.io/contrib/instrumentation/google.golang.org/grpc/otelgrpc"

	pb "observability2/go/pb"

	"observability2/go/internal/telemetry"
)

type greeterServer struct {
	pb.UnimplementedGreeterServer
}

func (s *greeterServer) SayHello(ctx context.Context, req *pb.HelloRequest) (*pb.HelloReply, error) {
	slog.InfoContext(ctx, "Received request", "name", req.GetName())
	return &pb.HelloReply{Message: "Hello, " + req.GetName() + "!"}, nil
}

func main() {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()

	shutdown, err := telemetry.Init(ctx)
	if err != nil {
		slog.Error("failed to initialize telemetry", "error", err)
		os.Exit(1)
	}
	defer func() {
		if err := shutdown(context.Background()); err != nil {
			slog.Error("telemetry shutdown error", "error", err)
		}
	}()

	lis, err := net.Listen("tcp", "[::]:50051")
	if err != nil {
		slog.Error("failed to listen", "error", err)
		os.Exit(1)
	}

	srv := grpc.NewServer(grpc.StatsHandler(otelgrpc.NewServerHandler()))
	pb.RegisterGreeterServer(srv, &greeterServer{})

	slog.Info("Server starting on port 50051")

	go func() {
		<-ctx.Done()
		slog.Info("Shutting down server")
		srv.GracefulStop()
	}()

	if err := srv.Serve(lis); err != nil {
		slog.Error("server error", "error", err)
		os.Exit(1)
	}
}
