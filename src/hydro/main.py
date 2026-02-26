from hydro.loggers import create_logger


logger = create_logger("Main")


def main() -> None:
    logger.info("---------- BEGIN LOGS -----------")


if __name__ == "__main__":
    main()