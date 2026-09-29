use crate::graphql::build_schema;
use crate::graphql::cost_analysis::{CostAnalysisConfig, QueryCostAnalyzer, QueryCostMetrics};
use actix_web::http::StatusCode;
use actix_web::{middleware, web, App, HttpServer};
use async_graphql_actix_web::{GraphQL, GraphQLRequest, GraphQLResponse};

pub async fn start_graphql_server(port: u16) -> std::io::Result<()> {
    start_graphql_server_with_config(port, CostAnalysisConfig::default_safe(), None).await
}

pub async fn start_graphql_server_with_config(
    port: u16,
    cost_config: CostAnalysisConfig,
    metrics: Option<QueryCostMetrics>,
) -> std::io::Result<()> {
    let schema = web::Data::new(build_schema());
    let cost_data = web::Data::new(cost_config);
    let metrics_data = web::Data::new(metrics.unwrap_or_default());

    println!("GraphQL server running on http://localhost:{}", port);
    println!("GraphQL playground: http://localhost:{}/", port);

    HttpServer::new(move || {
        App::new()
            .app_data(schema.clone())
            .app_data(cost_data.clone())
            .app_data(metrics_data.clone())
            .wrap(middleware::Logger::default())
            .wrap(middleware::Compress::default())
            .service(
                web::resource("/graphql")
                    .route(web::post().to(graphql_handler))
                    .route(web::get().to(graphql_playground)),
            )
            .service(web::resource("/graphql/metrics").route(web::get().to(graphql_metrics)))
            .service(web::resource("/").route(web::get().to(playground_html)))
            .default_service(web::route().to(not_found))
    })
    .bind(("127.0.0.1", port))?
    .run()
    .await
}

async fn graphql_handler(
    schema: web::Data<crate::graphql::schema::StarforgeSchema>,
    cost_config: web::Data<CostAnalysisConfig>,
    metrics: web::Data<QueryCostMetrics>,
    req: web::Json<async_graphql::Request>,
) -> actix_web::HttpResponse {
    let request = req.into_inner();
    let metrics_ref = if cost_config.enable_metrics {
        Some(metrics.as_ref())
    } else {
        None
    };

    if let Err(limit_err) = QueryCostAnalyzer::analyze(&request.query, &cost_config, metrics_ref) {
        return actix_web::HttpResponse::Ok().json(limit_err.to_graphql_error_response());
    }

    let response = schema.execute(request).await;
    actix_web::HttpResponse::Ok().json(response)
}

async fn graphql_metrics(
    metrics: web::Data<QueryCostMetrics>,
) -> actix_web::Result<actix_web::HttpResponse> {
    Ok(actix_web::HttpResponse::Ok().json(metrics.snapshot()))
}

async fn graphql_playground() -> actix_web::Result<actix_web::HttpResponse> {
    let html = r#"
    <!DOCTYPE html>
    <html>
    <head>
        <title>GraphQL Playground</title>
        <meta charset=utf-8/>
        <meta name="viewport" content="width=device-width, initial-scale=1"/>
        <link rel="stylesheet" href="//cdn.jsdelivr.net/npm/graphql-playground-react/build/static/css/index.css"/>
        <link rel="shortcut icon" href="//cdn.jsdelivr.net/npm/graphql-playground-react/build/favicon.png"/>
        <script src="//cdn.jsdelivr.net/npm/graphql-playground-react/build/umd/graphql-playground.js"></script>
    </head>
    <body>
        <div id="root"></div>
        <script>
            window.addEventListener('load', function (event) {
                GraphQLPlayground.init(document.getElementById('root'), {
                    endpoint: '/graphql',
                    subscriptionEndpoint: 'ws://localhost:8000/graphql',
                });
            });
        </script>
    </body>
    </html>
    "#;

    Ok(actix_web::HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html))
}

async fn playground_html() -> actix_web::Result<actix_web::HttpResponse> {
    graphql_playground().await
}

async fn not_found() -> actix_web::Result<actix_web::HttpResponse> {
    Ok(actix_web::HttpResponse::NotFound().json(serde_json::json!({
        "error": "Not Found"
    })))
}
